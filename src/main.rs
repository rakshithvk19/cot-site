mod guides;

use std::time::Duration;

use askama::Template;
use askama::filters::{HtmlSafe, Safe};
use cot::cli::CliMetadata;
use cot::config::{ProjectConfig, StaticFilesConfig, StaticFilesPathRewriteMode};
use cot::html::Html;
use cot::http::request::Parts;
use cot::project::{App, MiddlewareContext, Project, RegisterAppsContext, RootHandlerBuilder};
use cot::request::RequestExt;
use cot::request::extractors::{FromRequestParts, Path, StaticFiles};
use cot::response::{IntoResponse, Response};
use cot::router::{Route, Router, Urls};
use cot::static_files::{StaticFile, StaticFilesMiddleware};
use cot::{AppBuilder, BoxedHandler, reverse_redirect, static_files};
use cot_site_common::md_pages::{MdPage, MdPageLink, Section};
use cot_site_macros::md_page;

use crate::guides::{get_prev_next_link, parse_guides};

pub(crate) const LATEST_VERSION: &str = "v0.3";
pub(crate) const ALL_VERSIONS: &[&str] = &["latest", "v0.3", "v0.2", "v0.1"];

#[derive(Debug, Clone)]
struct BaseContext {
    urls: Urls,
    static_files: StaticFiles,
    route_name: String,
}

impl FromRequestParts for BaseContext {
    async fn from_request_parts(parts: &mut Parts) -> cot::Result<Self> {
        let urls = Urls::from_request_parts(parts).await?;
        let static_files = StaticFiles::from_request_parts(parts).await?;
        let route_name = parts.route_name().unwrap_or_default().to_owned();

        Ok(Self {
            urls,
            static_files,
            route_name,
        })
    }
}

#[derive(Debug, Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    base_context: &'a BaseContext,
}

async fn index(base_context: BaseContext) -> cot::Result<Html> {
    let index_template = IndexTemplate {
        base_context: &base_context,
    };
    let rendered = index_template.render()?;

    Ok(Html::new(rendered))
}

#[derive(Debug, Template)]
#[template(path = "guide.html")]
struct GuideTemplate<'a> {
    link_categories: &'a [GuideLinkCategory],
    guide: &'a MdPage,
    versions: &'static [&'static str],
    version: &'a str,
    base_context: &'a BaseContext,
    prev: Option<&'a MdPageLink>,
    next: Option<&'a MdPageLink>,
}

#[derive(Debug, Clone)]
struct GuideLinkCategory {
    title: &'static str,
    guides: Vec<MdPageLink>,
}

fn render_section(section: &Section) -> Safe<String> {
    #[derive(Debug, Clone, Template)]
    #[template(path = "_md_page_toc_item.html")]
    struct RenderableSection<'a> {
        section: &'a Section,
    }

    impl HtmlSafe for RenderableSection<'_> {}

    let rendered = RenderableSection { section }.render().unwrap();
    Safe(rendered)
}

const DEFAULT_GUIDE_PAGE: &str = "introduction";

async fn guide(base_context: BaseContext) -> cot::Result<Response> {
    reverse_redirect!(base_context.urls, "guide_version", version = "latest")
}

async fn guide_version(
    base_context: BaseContext,
    Path(version): Path<String>,
) -> cot::Result<Html> {
    page_response(base_context, &version, DEFAULT_GUIDE_PAGE)
}

async fn guide_page(
    base_context: BaseContext,
    Path((version, page)): Path<(String, String)>,
) -> cot::Result<Response> {
    if page == DEFAULT_GUIDE_PAGE {
        return Ok(reverse_redirect!(
            base_context.urls,
            "guide_version",
            version = version
        )?);
    }

    page_response(base_context, &version, &page).into_response()
}

fn page_response(base_context: BaseContext, version: &str, page: &str) -> cot::Result<Html> {
    let file_version = if version == "latest" {
        LATEST_VERSION
    } else {
        version
    };
    if !ALL_VERSIONS.contains(&file_version) {
        return Err(cot::Error::not_found());
    }
    let (link_categories, guide_map) = parse_guides(file_version);
    let guide = guide_map.get(page).ok_or_else(cot::Error::not_found)?;
    let (prev, next) = get_prev_next_link(&link_categories, page);

    let guide_template = GuideTemplate {
        link_categories: &link_categories,
        guide,
        versions: ALL_VERSIONS,
        version,
        base_context: &base_context,
        prev,
        next,
    };

    let rendered = guide_template.render()?;
    Ok(Html::new(rendered))
}

#[derive(Debug, Template)]
#[template(path = "md_page.html")]
struct MdPageTemplate<'a> {
    page: &'a MdPage,
    base_context: &'a BaseContext,
}

async fn faq(base_context: BaseContext) -> cot::Result<Html> {
    let template = MdPageTemplate {
        page: &md_page!("", "faq"),
        base_context: &base_context,
    };

    Ok(Html::new(template.render()?))
}

async fn licenses(base_context: BaseContext) -> cot::Result<Html> {
    let template = MdPageTemplate {
        page: &md_page!("", "licenses"),
        base_context: &base_context,
    };

    Ok(Html::new(template.render()?))
}

struct CotSiteApp;

impl App for CotSiteApp {
    fn name(&self) -> &'static str {
        "cot-site"
    }

    fn router(&self) -> Router {
        Router::with_urls([
            Route::with_handler_and_name("/", index, "index"),
            Route::with_handler_and_name("/faq/", faq, "faq"),
            Route::with_handler_and_name("/licenses/", licenses, "licenses"),
            Route::with_handler_and_name("/guide/", guide, "guide"),
            Route::with_handler_and_name("/guide/{version}/", guide_version, "guide_version"),
            Route::with_handler_and_name("/guide/{version}/{page}/", guide_page, "guide_page"),
        ])
    }

    fn static_files(&self) -> Vec<StaticFile> {
        static_files!(
            "favicon.ico",
            "static/css/main.css",
            "static/js/color-modes.js",
            "static/images/cot-dark.svg",
            "static/images/favicon.svg",
            "static/images/favicon-32.png",
            "static/images/favicon-180.png",
            "static/images/favicon-192.png",
            "static/images/favicon-512.png",
            "static/images/site.webmanifest"
        )
    }
}

struct CotSiteProject;

impl Project for CotSiteProject {
    fn cli_metadata(&self) -> CliMetadata {
        cot::cli::metadata!()
    }

    fn config(&self, _config_name: &str) -> cot::Result<ProjectConfig> {
        // we don't need to load any config
        Ok(ProjectConfig::builder()
            .static_files(
                StaticFilesConfig::builder()
                    .url("/")
                    .rewrite(StaticFilesPathRewriteMode::QueryParam)
                    .cache_timeout(Duration::from_secs(365 * 24 * 60 * 60))
                    .build(),
            )
            .build())
    }

    fn register_apps(&self, modules: &mut AppBuilder, _app_context: &RegisterAppsContext) {
        modules.register_with_views(CotSiteApp, "");
    }

    fn middlewares(
        &self,
        handler: RootHandlerBuilder,
        context: &MiddlewareContext,
    ) -> BoxedHandler {
        let handler = handler.middleware(StaticFilesMiddleware::from_context(context));
        #[cfg(debug_assertions)]
        let handler = handler.middleware(cot::middleware::LiveReloadMiddleware::new());
        handler.build()
    }
}

#[cot::main]
fn main() -> impl Project {
    CotSiteProject
}

pub mod arbitration;
pub mod capture;
pub mod ducanator;
pub mod foundry;
mod gamelog;
mod images;
pub mod inventory_view;
pub mod market;
pub mod mastery;
mod net;
mod overlay;
mod overview;
mod pipeline;
pub mod riven_data;
pub mod riven_grades;
pub mod riven_market;
pub mod rivens;
mod stats;
pub mod translate;
pub mod trigger;
pub mod update;
pub mod worldstate;

use std::sync::Arc;

use tauri::http::{Request, Response};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use images::{ImageService, Served};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // App updates from GitHub Releases (signed; see tauri.conf.json → plugins.updater).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(overlay::shortcut_plugin())
        // Reopens the main window at the size, position and maximized state it was closed with
        // (first launch: 1350 × 720, centred). The overlay window places itself over the game.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&[overlay::WINDOW])
                .build(),
        )
        .manage(Arc::new(pipeline::Shared::default()))
        // Item images: the page loads `http://wfimg.localhost/<imageName>` (see images.rs).
        .register_asynchronous_uri_scheme_protocol("wfimg", serve_image)
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                app.manage(ImageService::start(dir.join("images")));
                let shared = app.state::<Arc<pipeline::Shared>>().inner().clone();
                app.manage(market::Market::start(app.handle().clone(), shared, dir.join("market")));
                app.manage(ducanator::Ducanator::new(dir.join("market")));
            }
            pipeline::start(app.handle().clone());
            let shared = app.state::<Arc<pipeline::Shared>>().inner().clone();
            riven_data::start(app.handle());
            overlay::start(app.handle(), shared);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pipeline::get_status,
            pipeline::capture_now,
            overview::get_overview,
            foundry::get_foundry,
            foundry::set_favorite,
            inventory_view::get_inventory,
            market::request_prices,
            ducanator::get_ducanator,
            mastery::get_mastery,
            arbitration::get_arbitrations,
            worldstate::get_fissures,
            stats::get_currency_history,
            rivens::get_rivens,
            riven_data::riven_data_status,
            riven_data::check_riven_data,
            riven_market::similar_rivens,
            overlay::get_tool_settings,
            overlay::set_riven_overlay,
            overlay::preview_riven_overlay,
            overlay::set_relic_overlay,
            overlay::set_focus_game,
            overlay::set_debug_logging,
            overlay::debug_folder,
            overlay::preview_relic_overlay,
            overlay::overlay_state,
            overlay::overlay_done
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn serve_image<R: Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let name = net::decode_path_segment(request.uri().path().trim_start_matches('/'));
    let Some(service) = ctx.app_handle().try_state::<ImageService>() else {
        responder.respond(image_response(Served::Unavailable));
        return;
    };
    service.request(name, Box::new(move |served| responder.respond(image_response(served))));
}

fn image_response(served: Served) -> Response<Vec<u8>> {
    let builder = Response::builder().header("Access-Control-Allow-Origin", "*");
    let response = match served {
        // Cached files never change, so the page may keep them as long as it likes.
        Served::Image { bytes, mime } => builder
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(bytes),
        Served::NotFound => builder.status(404).body(Vec::new()),
        Served::Unavailable => builder.status(503).header("Cache-Control", "no-store").body(Vec::new()),
    };
    response.expect("static response parts are valid")
}

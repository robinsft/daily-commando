// SPDX-License-Identifier: EUPL-1.2
// Daily Commando – minimalist daily standup timer
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};
use webkit2gtk::{WebContext, WebView, WebViewExt};

const HTML: &str = include_str!("../frontend/index.html");

fn main() {
    let app = Application::builder()
        .application_id("io.github.robinsft.daily-commando")
        .build();

    app.connect_activate(|app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Daily Commando")
            .default_width(900)
            .default_height(640)
            .build();

        let ctx = WebContext::default().unwrap();
        let webview = WebView::with_context(&ctx);
        webview.load_html(HTML, None);

        window.set_child(Some(&webview));
        window.present();
    });

    app.run();
}

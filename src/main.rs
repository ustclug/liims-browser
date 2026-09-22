mod browser;
mod config;
mod home;
mod navigation;
mod session;

use adw::prelude::*;
use clap::Parser;
use gtk::{gio, glib};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Administrator configuration; otherwise /etc/liims/browser.toml or built-in defaults.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Campus profile; otherwise profile= from the kernel command line, then default.
    #[arg(long)]
    profile: Option<String>,
    /// Validate configuration without opening a window.
    #[arg(long)]
    check_config: bool,
    /// Start unmaximized with normal window controls for development.
    #[arg(long)]
    windowed: bool,
    /// Open an HTTP(S) page on startup (useful for site compatibility checks).
    #[arg(long)]
    url: Option<String>,
}

fn main() -> glib::ExitCode {
    let args = Args::parse();
    let result = (|| {
        let config = config::Config::load(args.config.as_deref())?;
        let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
        let profile_name = args
            .profile
            .as_deref()
            .or_else(|| config::boot_profile(&cmdline))
            .unwrap_or("default");
        let profile = config.profile(profile_name)?;
        if args
            .url
            .as_ref()
            .is_some_and(|url| !navigation::is_web_url(url))
        {
            return Err("--url 只接受 HTTP/HTTPS 地址".to_string());
        }
        Ok((config, profile))
    })();
    let (config, profile) = match result {
        Ok(result) => result,
        Err(error) => {
            eprintln!("配置错误：{error}");
            return glib::ExitCode::FAILURE;
        }
    };
    if args.check_config {
        println!(
            "配置有效：{}；空闲 {} 秒清理",
            profile.name, config.idle_seconds
        );
        return glib::ExitCode::SUCCESS;
    }
    // Each process owns its own ephemeral session. Service restarts cannot activate an old instance.
    let app = adw::Application::builder()
        .application_id("cn.edu.ustc.liims.Browser")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(include_str!("../data/style.css"));
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let browser = browser::Browser::new(app, config.clone(), profile.clone(), args.windowed);
        if let Some(uri) = &args.url {
            browser.navigate(uri);
        }
        browser.window.present();
        browser.keep_alive();
    });
    app.run_with_args::<&str>(&[])
}

use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

struct Server {
    url: String,
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let mut stream = stream.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut buffer = [0; 8192];
                let Ok(length) = stream.read(&mut buffer) else {
                    continue;
                };
                let request = String::from_utf8_lossy(&buffer[..length]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (status, headers, body) = match path {
                    "/redirect" => ("302 Found", "Location: file:///etc/passwd\r\n", ""),
                    "/download" => (
                        "200 OK",
                        "Content-Disposition: attachment; filename=secret.txt\r\n",
                        "private download",
                    ),
                    _ => (
                        "200 OK",
                        "",
                        "<!doctype html><meta charset=utf-8><title>本地测试页面</title><h1>馆藏查询测试</h1><p>用于验证会话隔离、页面查找与浏览器行为。</p><input id=query><input id=file type=file><a id=popup href='/child' target='_blank'>图书详情</a><a id=download href='/download' download>下载</a>",
                    ),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            url,
            stopped,
            thread: Some(thread),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(self.url.trim_start_matches("http://"));
        self.thread.take().unwrap().join().unwrap();
    }
}

fn spin_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "GUI condition timed out");
        glib::MainContext::default().iteration(false);
        thread::sleep(Duration::from_millis(5));
    }
}

fn settle() {
    let until = Instant::now() + Duration::from_millis(350);
    spin_until(|| Instant::now() >= until);
}

fn javascript(view: &webkit6::WebView, script: &str) -> String {
    let output = Rc::new(RefCell::new(None));
    let result = output.clone();
    view.evaluate_javascript(script, None, None, gio::Cancellable::NONE, move |value| {
        *result.borrow_mut() = Some(value.map(|v| v.to_str().to_string()));
    });
    spin_until(|| output.borrow().is_some());

    output
        .borrow_mut()
        .take()
        .unwrap()
        .expect("JavaScript execution failed")
}

fn loaded(browser: &Browser, uri: &str) -> webkit6::WebView {
    spin_until(|| {
        browser
            .selected_view()
            .is_some_and(|v| v.uri().as_deref() == Some(uri) && !v.is_loading())
    });
    let view = browser.selected_view().unwrap();
    spin_until(|| view.title().as_deref() == Some("本地测试页面"));
    view
}

fn screenshot(browser: &Browser, name: &str) {
    use gtk::gsk::prelude::*;
    let Ok(directory) = std::env::var("LIIMS_SCREENSHOT_DIR") else {
        return;
    };
    settle();
    eprintln!("Screenshot: {name}");
    let paintable = gtk::WidgetPaintable::new(Some(&browser.window));
    let mut node = None;
    spin_until(|| {
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(
            &snapshot,
            browser.window.width() as f64,
            browser.window.height() as f64,
        );
        node = snapshot.to_node();
        node.is_some()
    });
    let texture = browser
        .window
        .renderer()
        .unwrap()
        .render_texture(node.unwrap(), None);
    std::fs::create_dir_all(&directory).unwrap();
    texture
        .save_to_png(std::path::Path::new(&directory).join(format!("{name}.png")))
        .unwrap();
}

#[test]
#[ignore = "requires an isolated D-Bus session and a display; run scripts/gui-test.py"]
fn browser_session_and_policy() {
    adw::init().unwrap();
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../../data/style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let app = adw::Application::builder()
        .application_id("cn.edu.ustc.liims.Browser.Test")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let config = Config::parse(crate::config::DEFAULT_CONFIG).unwrap();
    let browser = Browser::new(
        &app,
        config.clone(),
        config.profile("default").unwrap(),
        true,
    );
    browser.window.present();
    spin_until(|| browser.window.is_mapped());
    assert_eq!(browser.tabs.n_pages(), 1);
    assert!(browser.session.borrow().is_ephemeral());
    screenshot(&browser, "home");
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    screenshot(&browser, "home-dark");
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
    browser.window.set_default_size(1024, 700);
    screenshot(&browser, "home-1024");
    browser.window.set_default_size(800, 700);
    screenshot(&browser, "home-narrow");
    browser.window.set_default_size(1366, 768);
    screenshot(&browser, "home-1366");
    browser.window.set_default_size(1024, 700);
    let server = Server::new();
    let initial = format!("{}/initial", server.url);
    browser.navigate(&initial);
    let first = loaded(&browser, &initial);
    assert_eq!(
        javascript(
            &first,
            "document.cookie='reader=first; path=/'; localStorage.setItem('reader', 'first'); sessionStorage.setItem('reader','first'); 'stored'"
        ),
        "stored"
    );
    javascript(
        &first,
        "window.dbReady=false; const request=indexedDB.open('reader',1); request.onupgradeneeded=()=>request.result.createObjectStore('data'); request.onsuccess=()=>{request.result.close();window.dbReady=true}; 'started'",
    );
    spin_until(|| javascript(&first, "window.dbReady") == "true");
    assert!(browser.clock.borrow().state(Instant::now()) == IdleState::Active);
    browser.location.grab_focus();
    browser.location.set_text("尚未提交的馆藏搜索");
    javascript(&first, "document.title='后台更新标题'; 'updated'");
    settle();
    assert_eq!(browser.location.text(), "尚未提交的馆藏搜索");

    browser.new_tab(None);
    assert_eq!(browser.location.text(), "");
    let second_uri = format!("{}/second", server.url);
    browser.navigate(&second_uri);
    let second = loaded(&browser, &second_uri);
    assert_eq!(
        javascript(&second, "localStorage.getItem('reader')"),
        "first"
    );
    assert_eq!(javascript(&second, "document.cookie"), "reader=first");
    assert_eq!(first.network_session(), second.network_session());
    javascript(
        &second,
        "document.getElementById('popup').click(); 'clicked'",
    );
    spin_until(|| browser.tabs.n_pages() == 3);
    let popup_uri = format!("{}/child", server.url);
    let popup = loaded(&browser, &popup_uri);
    assert_eq!(
        javascript(&popup, "localStorage.getItem('reader')"),
        "first"
    );
    browser.action("close-tab");
    assert_eq!(browser.tabs.n_pages(), 2);
    // Restore the second tab explicitly; tab selection after a close is toolkit-managed.
    let second_tab = browser.pages.borrow()[1].clone();
    browser.tabs.set_selected_page(&second_tab.page);
    let uploads = Rc::new(Cell::new(0));
    second.connect_run_file_chooser({
        let uploads = uploads.clone();
        move |_, _| {
            uploads.set(uploads.get() + 1);
            false
        }
    });
    javascript(
        &second,
        "document.getElementById('file').click(); 'clicked'",
    );
    settle();
    // Our handler stops signal propagation, so the fallback handler is never reached.
    assert_eq!(uploads.get(), 0);
    javascript(&second, "window.print(); 'printed'");
    assert!(browser.window.visible_dialog().is_none());
    let download = second
        .download_uri(&format!("{}/download", server.url))
        .unwrap();
    let cancelled = Rc::new(Cell::new(false));
    download.connect_failed({
        let cancelled = cancelled.clone();
        move |_, error| cancelled.set(error.matches(webkit6::DownloadError::CancelledByUser))
    });
    spin_until(|| cancelled.get());
    browser.action("find");
    browser.find_entry.set_text("查询");
    settle();
    assert!(browser.find_bar.is_search_mode());
    browser.close_find();
    browser.action("zoom-in");
    assert!(second.zoom_level() > 1.0);
    browser.action("zoom-reset");
    assert_eq!(second.zoom_level(), 1.0);
    screenshot(&browser, "tabs");
    browser.overview.set_open(true);
    screenshot(&browser, "overview");
    browser.overview.set_open(false);

    let first_tab = browser.pages.borrow()[0].clone();
    browser.tabs.close_page(&first_tab.page);
    browser.action("home");
    assert_eq!(browser.tabs.n_pages(), 1);
    assert!(browser.selected_view().is_none());
    browser.navigate(&initial);
    let before_reset = loaded(&browser, &initial);
    assert_eq!(
        javascript(&before_reset, "localStorage.getItem('reader')"),
        "first"
    );
    browser.action("close-tab");
    assert_eq!(browser.tabs.n_pages(), 1);
    assert!(browser.selected_view().is_none());

    browser.navigate(&initial);
    let old_tab = browser.selected_tab().unwrap();
    let old_view = loaded(&browser, &initial);
    let old_session = browser.session.borrow().clone();
    browser.confirm_reset();
    screenshot(&browser, "end-session");
    browser.reset_session();
    assert_eq!(browser.tabs.n_pages(), 1);
    assert!(browser.selected_view().is_none());
    assert_ne!(*browser.session.borrow(), old_session);
    assert!(!browser.is_current(&old_tab));
    // Even an intentionally retained old reference must not affect the new session.
    browser.load_in_tab(&old_tab, &second_uri);
    assert!(browser.selected_view().is_none());
    drop(old_view);
    drop(old_tab);
    drop(old_session);
    drop(first);
    drop(second);
    drop(before_reset);
    browser.navigate(&initial);
    let fresh = loaded(&browser, &initial);
    assert_eq!(javascript(&fresh, "document.cookie"), "");
    assert_eq!(javascript(&fresh, "localStorage.getItem('reader')"), "null");
    assert_eq!(
        javascript(&fresh, "sessionStorage.getItem('reader')"),
        "null"
    );
    javascript(
        &fresh,
        "window.databaseCount=-1; indexedDB.databases().then(d=>window.databaseCount=d.length); 'started'",
    );
    spin_until(|| javascript(&fresh, "window.databaseCount") != "-1");
    assert_eq!(javascript(&fresh, "window.databaseCount"), "0");
    assert!(!fresh.can_go_back());

    javascript(
        &fresh,
        "setTimeout(()=>window.confirmed=confirm('测试确认'),0); 'scheduled'",
    );
    spin_until(|| browser.window.visible_dialog().is_some());
    let dialog = browser
        .selected_tab()
        .unwrap()
        .dialogs
        .borrow()
        .last()
        .unwrap()
        .clone();
    dialog.set_close_response("ok");
    dialog.close();
    spin_until(|| javascript(&fresh, "window.confirmed") == "true");
    javascript(
        &fresh,
        "setTimeout(()=>alert('清理时应关闭此提示'),0); 'scheduled'",
    );
    spin_until(|| browser.window.visible_dialog().is_some());
    browser.reset_session();
    settle();
    assert!(browser.window.visible_dialog().is_none());
    browser.navigate(&initial);
    let fresh = loaded(&browser, &initial);

    fresh.load_uri("file:///etc/passwd");
    settle();
    assert_ne!(fresh.uri().as_deref(), Some("file:///etc/passwd"));
    assert!(!javascript(&fresh, "document.body.innerText").contains("root:x:"));
    browser.navigate(&format!("{}/redirect", server.url));
    settle();
    assert!(!javascript(&fresh, "document.body.innerText").contains("root:x:"));

    let now = Instant::now();
    browser
        .clock
        .borrow_mut()
        .start(now - Duration::from_secs(46));
    settle();
    assert!(browser.idle_banner.is_revealed());
    screenshot(&browser, "idle-warning");
    browser.activity();
    settle();
    assert!(!browser.idle_banner.is_revealed());
    browser
        .clock
        .borrow_mut()
        .start(Instant::now() - Duration::from_secs(61));
    spin_until(|| browser.selected_view().is_none());
    assert_eq!(
        browser.clock.borrow().state(Instant::now()),
        IdleState::Inactive
    );
    if let Ok(report) = std::env::var("LIIMS_PROBE_SITES") {
        probe_sites(&browser, &report);
    }
    browser.window.close();
    drop(browser);
    settle();
}

fn probe_sites(browser: &Rc<Browser>, report_path: &str) {
    let mut report = String::from("profile\tentry\tstatus\thttp\trequested\tfinal\ttitle\n");
    for (profile_name, profile) in &browser.config.profiles {
        for (index, link) in profile.links.iter().enumerate() {
            browser.reset_session();
            browser.navigate(&link.url);
            let view = browser.selected_view().unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while view.is_loading() && Instant::now() < deadline {
                glib::MainContext::default().iteration(false);
                thread::sleep(Duration::from_millis(10));
            }
            let loaded = browser
                .selected_tab()
                .unwrap()
                .stack
                .visible_child_name()
                .as_deref()
                == Some("web");
            let status = if view.is_loading() {
                "timeout"
            } else if loaded {
                "loaded"
            } else {
                "error"
            };
            let http = view
                .main_resource()
                .and_then(|r| r.response())
                .map(|r| r.status_code())
                .unwrap_or(0);
            let final_uri = view.uri().unwrap_or_default();
            let title = view.title().unwrap_or_default().replace(['\t', '\n'], " ");
            report.push_str(&format!(
                "{profile_name}\t{}\t{status}\t{http}\t{}\t{final_uri}\t{title}\n",
                link.title, link.url
            ));
            eprintln!(
                "Site: {profile_name}/{}: {status}, HTTP {http}, {title}",
                link.title
            );
            view.stop_loading();
            screenshot(browser, &format!("site-{profile_name}-{index}"));
        }
    }
    std::fs::write(report_path, report).unwrap();
}

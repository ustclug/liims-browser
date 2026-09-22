use crate::{
    config::{Config, Profile},
    home, navigation,
    session::{ActivityClock, IdleState},
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use webkit6::prelude::*;

mod dialogs;
#[cfg(test)]
mod tests;
mod web;

struct Tab {
    page: adw::TabPage,
    stack: gtk::Stack,
    view: RefCell<Option<webkit6::WebView>>,
    generation: u64,
    last_uri: RefCell<String>,
    dialogs: RefCell<Vec<adw::AlertDialog>>,
}

pub struct Browser {
    pub window: adw::ApplicationWindow,
    config: Config,
    profile: Profile,
    tabs: adw::TabView,
    pages: RefCell<Vec<Rc<Tab>>>,
    overview: adw::TabOverview,
    location: gtk::Entry,
    suggestion: gtk::Label,
    back: gtk::Button,
    forward: gtk::Button,
    reload: gtk::Button,
    progress: gtk::ProgressBar,
    idle_banner: adw::Banner,
    hint_banner: adw::Banner,
    toast: adw::ToastOverlay,
    active_toast: RefCell<Option<adw::Toast>>,
    find_bar: gtk::SearchBar,
    find_entry: gtk::SearchEntry,
    session: RefCell<webkit6::NetworkSession>,
    clock: RefCell<ActivityClock>,
    generation: Cell<u64>,
    resetting: Cell<bool>,
    hint_shown: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
    reset_dialog: RefCell<Option<adw::AlertDialog>>,
    dialogs: RefCell<Vec<adw::AlertDialog>>,
    syncing_location: Cell<bool>,
}

impl Browser {
    pub fn new(
        app: &adw::Application,
        config: Config,
        profile: Profile,
        windowed: bool,
    ) -> Rc<Self> {
        let builder =
            gtk::Builder::from_string(include_str!(concat!(env!("OUT_DIR"), "/window.ui")));
        let window: adw::ApplicationWindow = builder.object("window").unwrap();
        let header: adw::HeaderBar = builder.object("header").unwrap();
        let tabs: adw::TabView = builder.object("tabs").unwrap();
        let overview: adw::TabOverview = builder.object("overview").unwrap();
        let location: gtk::Entry = builder.object("location").unwrap();
        let suggestion: gtk::Label = builder.object("suggestion").unwrap();
        let back: gtk::Button = builder.object("back").unwrap();
        let forward: gtk::Button = builder.object("forward").unwrap();
        let reload: gtk::Button = builder.object("reload").unwrap();
        let progress: gtk::ProgressBar = builder.object("progress").unwrap();
        let idle_banner: adw::Banner = builder.object("idle_banner").unwrap();
        let hint_banner: adw::Banner = builder.object("hint_banner").unwrap();
        let toast: adw::ToastOverlay = builder.object("toast").unwrap();
        let find_bar: gtk::SearchBar = builder.object("find_bar").unwrap();
        let find_entry: gtk::SearchEntry = builder.object("find_entry").unwrap();
        window.set_application(Some(app));
        header.set_show_start_title_buttons(windowed);
        header.set_show_end_title_buttons(windowed);
        overview.set_show_start_title_buttons(windowed);
        overview.set_show_end_title_buttons(windowed);

        let browser = Rc::new(Self {
            window,
            clock: RefCell::new(ActivityClock::new(
                config.idle_seconds,
                config.warning_seconds,
            )),
            config,
            profile,
            tabs,
            pages: RefCell::new(Vec::new()),
            overview,
            location,
            suggestion,
            back,
            forward,
            reload,
            progress,
            idle_banner,
            hint_banner,
            toast,
            active_toast: RefCell::new(None),
            find_bar,
            find_entry,
            session: RefCell::new(Self::new_session()),
            generation: Cell::new(0),
            resetting: Cell::new(false),
            hint_shown: Cell::new(false),
            timer: RefCell::new(None),
            reset_dialog: RefCell::new(None),
            dialogs: RefCell::new(Vec::new()),
            syncing_location: Cell::new(false),
        });
        browser.connect_ui();
        browser.install_actions(app);
        browser.connect_session();
        browser.new_tab(None);
        if !windowed {
            browser.window.maximize();
        }
        browser
    }

    pub fn keep_alive(self: &Rc<Self>) {
        let owner = RefCell::new(Some(self.clone()));
        self.window.connect_unrealize(move |_| {
            owner.borrow_mut().take();
        });
    }

    fn new_session() -> webkit6::NetworkSession {
        let session = webkit6::NetworkSession::new_ephemeral();
        session.set_persistent_credential_storage_enabled(false);
        session
    }

    fn connect_session(self: &Rc<Self>) {
        let browser = self;
        self.session.borrow().connect_download_started(glib::clone!(
            #[weak]
            browser,
            move |session, download| {
                download.cancel();
                if *session == *browser.session.borrow() {
                    browser.notify("此查询终端不支持下载文件。");
                }
            }
        ));
    }

    fn notify(&self, message: &str) {
        let previous = self.active_toast.borrow_mut().take();
        if let Some(previous) = previous {
            previous.dismiss();
        }
        let toast = adw::Toast::new(message);
        self.toast.add_toast(toast.clone());
        *self.active_toast.borrow_mut() = Some(toast);
    }

    fn activity(&self) {
        self.clock.borrow_mut().activity(Instant::now());
        self.idle_banner.set_revealed(false);
    }

    fn start_activity(&self) {
        self.clock.borrow_mut().start(Instant::now());
        self.idle_banner.set_revealed(false);
    }

    fn connect_ui(self: &Rc<Self>) {
        let browser = self;
        self.location.connect_activate(glib::clone!(
            #[weak]
            browser,
            move |entry| {
                match navigation::resolve_input(&entry.text(), &browser.profile.search_url) {
                    Ok(uri) => browser.navigate(&uri),
                    Err(error) => browser.notify(error),
                }
            }
        ));
        self.location.connect_changed(glib::clone!(
            #[weak]
            browser,
            move |entry| {
                if browser.syncing_location.get() {
                    return;
                }
                browser.start_activity();
                let text = entry.text();
                let show = !text.trim().is_empty()
                    && navigation::resolve_input(&text, &browser.profile.search_url).is_ok_and(
                        |uri| uri == navigation::search_url(&browser.profile.search_url, &text),
                    );
                browser
                    .suggestion
                    .set_label(&format!("搜索馆藏：{}", text.trim()));
                browser.suggestion.set_visible(show);
            }
        ));
        self.tabs.connect_selected_page_notify(glib::clone!(
            #[weak]
            browser,
            move |_| {
                browser.close_find();
                browser.sync_location(true);
                browser.sync_toolbar();
            }
        ));
        self.tabs.connect_close_page(glib::clone!(
            #[weak]
            browser,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |view, page| {
                // Finish any overview transition before removing its thumbnail.
                // libadwaita 1.9 keeps a borrowed thumbnail during that animation.
                // Both visibility changes happen in this main-loop iteration.
                let visible = browser.overview.is_visible();
                browser.overview.set_visible(false);
                let removed = {
                    let mut pages = browser.pages.borrow_mut();
                    pages
                        .iter()
                        .position(|tab| tab.page == *page)
                        .map(|i| pages.remove(i))
                };
                if let Some(tab) = removed {
                    browser.dispose_tab(&tab);
                }
                view.close_page_finish(page, true);
                if view.n_pages() == 0 && !browser.resetting.get() {
                    browser.new_tab(None);
                }
                browser.overview.set_visible(visible);
                browser.sync_toolbar();
                glib::Propagation::Stop
            }
        ));
        self.overview.connect_create_tab(glib::clone!(
            #[weak]
            browser,
            #[upgrade_or_panic]
            move |_| browser.new_tab(None).page.clone()
        ));
        self.idle_banner.connect_button_clicked(glib::clone!(
            #[weak]
            browser,
            move |_| browser.activity()
        ));
        self.hint_banner.connect_button_clicked(glib::clone!(
            #[weak]
            browser,
            move |_| browser.hint_banner.set_revealed(false)
        ));
        self.find_entry.connect_search_changed(glib::clone!(
            #[weak]
            browser,
            move |entry| {
                if let Some(view) = browser.selected_view()
                    && let Some(find) = view.find_controller()
                {
                    if entry.text().is_empty() {
                        find.search_finish();
                    } else {
                        find.search(
                            &entry.text(),
                            (webkit6::FindOptions::CASE_INSENSITIVE
                                | webkit6::FindOptions::WRAP_AROUND)
                                .bits(),
                            1000,
                        );
                    }
                }
            }
        ));
        self.find_entry.connect_stop_search(glib::clone!(
            #[weak]
            browser,
            move |_| browser.close_find()
        ));
        self.find_bar
            .connect_search_mode_enabled_notify(glib::clone!(
                #[weak]
                browser,
                move |bar| {
                    if !bar.is_search_mode() {
                        browser.close_find();
                    }
                }
            ));
        let events = gtk::EventControllerLegacy::new();
        events.set_propagation_phase(gtk::PropagationPhase::Capture);
        let pointer_position = Cell::new(None);
        events.connect_event(glib::clone!(
            #[weak]
            browser,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                if event.event_type() == gdk::EventType::MotionNotify {
                    let position = event.position();
                    if pointer_position.replace(position) != position {
                        browser.activity();
                    }
                    return glib::Propagation::Proceed;
                }
                if matches!(
                    event.event_type(),
                    gdk::EventType::KeyPress
                        | gdk::EventType::KeyRelease
                        | gdk::EventType::ButtonPress
                        | gdk::EventType::ButtonRelease
                        | gdk::EventType::Scroll
                        | gdk::EventType::TouchBegin
                        | gdk::EventType::TouchUpdate
                ) {
                    browser.activity();
                }
                glib::Propagation::Proceed
            }
        ));
        self.window.add_controller(events);
        let timer = glib::timeout_add_local(
            Duration::from_millis(250),
            glib::clone!(
                #[weak]
                browser,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    let state = browser.clock.borrow().state(Instant::now());
                    match state {
                        IdleState::Warning(seconds) => {
                            browser
                                .idle_banner
                                .set_title(&format!("{seconds} 秒后将清理本次浏览并返回首页。"));
                            browser.idle_banner.set_revealed(true);
                        }
                        IdleState::Expired => browser.reset_session(),
                        _ => browser.idle_banner.set_revealed(false),
                    }
                    glib::ControlFlow::Continue
                }
            ),
        );
        *self.timer.borrow_mut() = Some(timer);
    }

    fn install_actions(self: &Rc<Self>, app: &adw::Application) {
        let actions: &[(&str, &[&str])] = &[
            ("new-tab", &["<Control>t"]),
            ("close-tab", &["<Control>w"]),
            ("back", &["<Alt>Left"]),
            ("forward", &["<Alt>Right"]),
            ("reload", &["<Control>r", "F5"]),
            ("home", &["<Alt>Home"]),
            ("location", &["<Control>l"]),
            ("find", &["<Control>f"]),
            ("find-next", &["<Control>g"]),
            ("find-previous", &["<Control><Shift>g"]),
            ("zoom-in", &["<Control>plus", "<Control>equal"]),
            ("zoom-out", &["<Control>minus"]),
            ("zoom-reset", &["<Control>0"]),
            ("overview", &["<Control><Shift>o"]),
            ("end-session", &[]),
            ("help", &["F1"]),
            ("about", &[]),
        ];
        for &(name, shortcuts) in actions {
            let action = gio::SimpleAction::new(name, None);
            let browser = self;
            action.connect_activate(glib::clone!(
                #[weak]
                browser,
                move |action, _| {
                    browser.activity();
                    browser.action(&action.name());
                }
            ));
            self.window.add_action(&action);
            app.set_accels_for_action(&format!("win.{name}"), shortcuts);
        }
    }

    fn action(self: &Rc<Self>, name: &str) {
        match name {
            "new-tab" => { self.new_tab(None); }
            "close-tab" => { if let Some(tab) = self.selected_tab() { self.tabs.close_page(&tab.page); } }
            "home" => { if let Some(tab) = self.selected_tab() { self.dispose_view(&tab); tab.stack.set_visible_child_name("home"); self.sync_toolbar(); } }
            "location" => { self.location.grab_focus(); self.location.select_region(0, -1); }
            "overview" => self.overview.set_open(!self.overview.is_open()),
            "end-session" => self.confirm_reset(),
            "help" => self.show_message("使用帮助", &format!("在首页输入书名，或在地址栏输入网址。\n\nCtrl + 空格：切换输入法\nCtrl + L：编辑网址\nCtrl + T / Ctrl + W：新建 / 关闭标签\nCtrl + F：页内查找\n\n回首页会保留本次会话。使用完毕请点击“结束使用”。连续 {} 秒没有操作也会自动清理。\n\n本终端不支持上传、下载和打印。", self.config.idle_seconds)),
            "about" => self.show_message("LIIMS Browser", &format!("{}\n公共图书查询终端\nRust · GTK4 · libadwaita · WebKitGTK\n\n由 USTC Linux User Group 维护。", env!("CARGO_PKG_VERSION"))),
            _ => {
                if let Some(view) = self.selected_view() {
                    match name {
                        "back" => view.go_back(), "forward" => view.go_forward(),
                        "reload" => { if view.is_loading() { view.stop_loading(); } else { view.reload(); } }
                        "find" => { self.find_bar.set_search_mode(true); self.find_entry.grab_focus(); }
                        "find-next" => { if let Some(find) = view.find_controller() { find.search_next(); } }
                        "find-previous" => { if let Some(find) = view.find_controller() { find.search_previous(); } }
                        "zoom-in" => view.set_zoom_level((view.zoom_level() + 0.1).min(3.0)),
                        "zoom-out" => view.set_zoom_level((view.zoom_level() - 0.1).max(0.5)),
                        "zoom-reset" => view.set_zoom_level(1.0), _ => (),
                    }
                }
            }
        }
    }

    fn close_find(&self) {
        if let Some(view) = self.selected_view()
            && let Some(find) = view.find_controller()
        {
            find.search_finish();
        }
        if self.find_bar.is_search_mode() {
            self.find_bar.set_search_mode(false);
        }
        self.find_entry.set_text("");
    }

    fn show_message(&self, title: &str, text: &str) {
        let dialog = Self::message_dialog(title, text);
        dialog.add_response("close", "知道了");
        dialog.set_default_response(Some("close"));
        dialog.set_close_response("close");
        dialog.present(Some(&self.window));
        self.dialogs.borrow_mut().retain(|d| d.is_visible());
        self.dialogs.borrow_mut().push(dialog);
    }

    fn confirm_reset(self: &Rc<Self>) {
        if self.reset_dialog.borrow().is_some() {
            return;
        }
        let dialog = Self::message_dialog(
            "结束本次使用？",
            "将关闭所有标签，并清除这台终端上的本次浏览数据。",
        );
        dialog.add_responses(&[("cancel", "继续使用"), ("reset", "结束并清理")]);
        dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let browser = self;
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak]
                browser,
                move |_, response| {
                    browser.reset_dialog.borrow_mut().take();
                    if response == "reset" {
                        browser.reset_session();
                    }
                }
            ),
        );
        *self.reset_dialog.borrow_mut() = Some(dialog.clone());
        dialog.present(Some(&self.window));
    }

    fn selected_tab(&self) -> Option<Rc<Tab>> {
        let page = self.tabs.selected_page()?;
        self.pages
            .borrow()
            .iter()
            .find(|tab| tab.page == page)
            .cloned()
    }

    fn selected_view(&self) -> Option<webkit6::WebView> {
        self.selected_tab()
            .and_then(|tab| tab.view.borrow().clone())
    }

    fn new_tab(self: &Rc<Self>, related: Option<&webkit6::WebView>) -> Rc<Tab> {
        let stack = gtk::Stack::builder().vexpand(true).build();
        let page = self.tabs.append(&stack);
        page.set_title("图书馆查询");
        page.set_icon(Some(&gio::ThemedIcon::new("go-home-symbolic")));
        let tab = Rc::new(Tab {
            page,
            stack,
            view: RefCell::new(None),
            generation: self.generation.get(),
            last_uri: RefCell::new(String::new()),
            dialogs: RefCell::new(Vec::new()),
        });
        let weak_browser = Rc::downgrade(self);
        let weak_tab = Rc::downgrade(&tab);
        let navigate = Rc::new(move |uri: String| {
            if let (Some(browser), Some(tab)) = (weak_browser.upgrade(), weak_tab.upgrade()) {
                browser.load_in_tab(&tab, &uri);
            }
        });
        let weak_browser = Rc::downgrade(self);
        let activity = Rc::new(move || {
            if let Some(browser) = weak_browser.upgrade() {
                browser.start_activity();
            }
        });
        tab.stack.add_named(
            &home::build(&self.profile, navigate, activity),
            Some("home"),
        );
        self.pages.borrow_mut().push(tab.clone());
        if let Some(view) = related {
            self.ensure_view(&tab, Some(view));
        }
        self.tabs.set_selected_page(&tab.page);
        self.sync_toolbar();
        tab
    }

    pub fn navigate(self: &Rc<Self>, uri: &str) {
        if let Some(tab) = self.selected_tab() {
            self.load_in_tab(&tab, uri);
        }
    }

    fn load_in_tab(self: &Rc<Self>, tab: &Rc<Tab>, uri: &str) {
        if !self.is_current(tab) {
            return;
        }
        if !navigation::is_web_url(uri) {
            self.notify("此终端只支持 HTTP 和 HTTPS 网页。");
            return;
        }
        self.start_activity();
        self.overview.set_open(false);
        let view = self.ensure_view(tab, None);
        *tab.last_uri.borrow_mut() = uri.to_owned();
        tab.stack.set_visible_child_name("web");
        view.load_uri(uri);
        view.grab_focus();
        self.suggestion.set_visible(false);
    }

    fn is_current(&self, tab: &Tab) -> bool {
        !self.resetting.get()
            && tab.generation == self.generation.get()
            && self
                .pages
                .borrow()
                .iter()
                .any(|current| current.page == tab.page)
    }

    fn is_current_view(&self, tab: &Tab, view: &webkit6::WebView) -> bool {
        self.is_current(tab) && tab.view.borrow().as_ref() == Some(view)
    }

    fn show_error(self: &Rc<Self>, tab: &Rc<Tab>, uri: &str, title: &str, description: &str) {
        *tab.last_uri.borrow_mut() = uri.into();
        if let Some(old) = tab.stack.child_by_name("error") {
            tab.stack.remove(&old);
        }
        let builder =
            gtk::Builder::from_string(include_str!(concat!(env!("OUT_DIR"), "/error.ui")));
        let status: adw::StatusPage = builder.object("error").unwrap();
        status.set_title(title);
        status.set_description(Some(description));
        let retry: gtk::Button = builder.object("retry").unwrap();
        let browser = self;
        retry.connect_clicked(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            move |_| {
                let uri = tab.last_uri.borrow().clone();
                browser.load_in_tab(&tab, &uri);
            }
        ));
        tab.stack.add_named(&status, Some("error"));
        tab.stack.set_visible_child_name("error");
        tab.page.set_title(title);
        tab.page.set_loading(false);
        self.sync_toolbar();
    }

    fn sync_toolbar(&self) {
        let view = self.selected_view();
        self.sync_location(false);
        self.back
            .set_sensitive(view.as_ref().is_some_and(|v| v.can_go_back()));
        self.forward
            .set_sensitive(view.as_ref().is_some_and(|v| v.can_go_forward()));
        self.reload.set_sensitive(view.is_some());
        let loading = view.as_ref().is_some_and(|v| v.is_loading());
        self.reload.set_icon_name(if loading {
            "process-stop-symbolic"
        } else {
            "view-refresh-symbolic"
        });
        self.progress.set_visible(loading);
        self.progress
            .set_fraction(view.map(|v| v.estimated_load_progress()).unwrap_or(0.0));
    }

    fn sync_location(&self, force: bool) {
        if force {
            self.suggestion.set_visible(false);
        }
        let view = self.selected_view();
        let uri = view.as_ref().and_then(|v| v.uri()).unwrap_or_default();
        self.syncing_location.set(true);
        // Do not replace text while the user edits the address.
        let editing = gtk::prelude::GtkWindowExt::focus(&self.window)
            .is_some_and(|focus| focus == self.location || focus.is_ancestor(&self.location));
        if force || !editing {
            self.location.set_text(&uri);
        }
        self.syncing_location.set(false);
    }

    fn dispose_view(&self, tab: &Tab) {
        let dialogs = std::mem::take(&mut *tab.dialogs.borrow_mut());
        for dialog in dialogs {
            dialog.force_close();
        }
        let view = tab.view.borrow_mut().take();
        if let Some(view) = view {
            view.stop_loading();
            if let Some(find) = view.find_controller() {
                find.search_finish();
            }
            tab.stack.remove(&view);
        }
        tab.page.set_loading(false);
        tab.page.set_title("图书馆查询");
        tab.page
            .set_icon(Some(&gio::ThemedIcon::new("go-home-symbolic")));
        tab.last_uri.borrow_mut().clear();
    }

    fn dispose_tab(&self, tab: &Tab) {
        self.dispose_view(tab);
    }

    fn reset_session(self: &Rc<Self>) {
        if self.resetting.replace(true) {
            return;
        }
        self.generation.set(self.generation.get() + 1);
        let notification = self.active_toast.borrow_mut().take();
        if let Some(notification) = notification {
            notification.dismiss();
        }
        self.overview.set_open(false);
        self.close_find();
        let dialog = self.reset_dialog.borrow_mut().take();
        if let Some(dialog) = dialog {
            dialog.force_close();
        }
        for dialog in self.dialogs.borrow_mut().drain(..) {
            dialog.force_close();
        }
        let pages = self.pages.borrow().clone();
        for tab in pages {
            self.tabs.close_page(&tab.page);
        }
        // Old views are unparented before dropping the session. No data is copied to the new session.
        *self.session.borrow_mut() = Self::new_session();
        self.connect_session();
        self.clock.borrow_mut().reset();
        self.hint_shown.set(false);
        self.hint_banner.set_revealed(false);
        self.idle_banner.set_revealed(false);
        self.syncing_location.set(true);
        self.location.set_text("");
        self.syncing_location.set(false);
        self.suggestion.set_visible(false);
        self.resetting.set(false);
        self.new_tab(None);
        self.notify("本次浏览数据已清理。");
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }
    }
}

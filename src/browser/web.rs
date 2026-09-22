use super::*;

impl Browser {
    pub(super) fn ensure_view(
        self: &Rc<Self>,
        tab: &Rc<Tab>,
        related: Option<&webkit6::WebView>,
    ) -> webkit6::WebView {
        if let Some(view) = tab.view.borrow().as_ref() {
            return view.clone();
        }
        let settings = webkit6::Settings::new();
        settings.set_enable_developer_extras(false);
        settings.set_enable_write_console_messages_to_stdout(false);
        settings.set_javascript_can_open_windows_automatically(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
        let manager = webkit6::UserContentManager::new();
        for rule in &self.config.site_rules {
            if rule.disable_synthetic_bold {
                let pattern = format!("*://{}{}*", rule.host, rule.path_prefix);
                manager.add_style_sheet(&webkit6::UserStyleSheet::new(
                    "* { font-synthesis: style; }",
                    webkit6::UserContentInjectedFrames::TopFrame,
                    webkit6::UserStyleLevel::User,
                    &[&pattern],
                    &[],
                ));
            }
        }
        let mut builder = webkit6::WebView::builder()
            .settings(&settings)
            .user_content_manager(&manager);
        if let Some(related) = related {
            builder = builder.related_view(related);
        } else {
            builder = builder.network_session(&self.session.borrow());
        }
        let view = builder.build();
        tab.page
            .set_icon(Some(&gio::ThemedIcon::new("text-html-symbolic")));
        self.connect_webview(tab, &view);
        self.connect_web_dialogs(tab, &view);
        tab.stack.add_named(&view, Some("web"));
        *tab.view.borrow_mut() = Some(view.clone());
        view
    }

    fn connect_webview(self: &Rc<Self>, tab: &Rc<Tab>, view: &webkit6::WebView) {
        let browser = self;
        if let Some(context) = view.input_method_context() {
            let composing = Rc::new(Cell::new(false));
            context.connect_preedit_started(glib::clone!(
                #[strong]
                composing,
                #[weak]
                browser,
                move |_| {
                    composing.set(true);
                    browser.activity();
                }
            ));
            context.connect_preedit_changed(glib::clone!(
                #[strong]
                composing,
                #[weak]
                browser,
                move |_| {
                    if composing.get() {
                        browser.activity();
                    }
                }
            ));
            context.connect_preedit_finished(move |_| composing.set(false));
            context.connect_committed(glib::clone!(
                #[weak]
                browser,
                move |_, text| {
                    if !text.is_empty() {
                        browser.activity();
                    }
                }
            ));
        }
        view.connect_decide_policy(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            true,
            move |view, decision, kind| {
                if !browser.is_current_view(&tab, view) {
                    decision.ignore();
                    return true;
                }
                if matches!(
                    kind,
                    webkit6::PolicyDecisionType::NavigationAction
                        | webkit6::PolicyDecisionType::NewWindowAction
                ) {
                    if let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
                    {
                        let uri = nav
                            .navigation_action()
                            .and_then(|action| action.request())
                            .and_then(|request| request.uri());
                        // about:blank is needed for script-created SSO windows; never accepted in the address bar.
                        if uri
                            .as_deref()
                            .is_some_and(|uri| uri != "about:blank" && !navigation::is_web_url(uri))
                        {
                            decision.ignore();
                            browser.notify("此终端只支持 HTTP 和 HTTPS 网页。");
                            return true;
                        }
                    }
                } else if kind == webkit6::PolicyDecisionType::Response
                    && let Some(response) =
                        decision.downcast_ref::<webkit6::ResponsePolicyDecision>()
                    && !response.is_mime_type_supported()
                {
                    decision.ignore();
                    browser.notify("此查询终端不支持下载文件。");
                    return true;
                }
                false
            }
        ));
        view.connect_create(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            None,
            move |view, action| {
                if !browser.is_current_view(&tab, view) {
                    return None;
                }
                if action
                    .request()
                    .and_then(|r| r.uri())
                    .as_deref()
                    .is_some_and(|uri| uri != "about:blank" && !navigation::is_web_url(uri))
                {
                    return None;
                }
                let child = browser.new_tab(Some(view));
                child.stack.set_visible_child_name("web");

                child.view.borrow().clone().map(|v| v.upcast())
            }
        ));
        view.connect_close(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            move |view| {
                if browser.is_current_view(&tab, view) {
                    browser.tabs.close_page(&tab.page);
                }
            }
        ));
        view.connect_load_changed(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            move |view, event| {
                if !browser.is_current_view(&tab, view) {
                    return;
                }
                if event == webkit6::LoadEvent::Started {
                    tab.stack.set_visible_child_name("web");
                }
                if event == webkit6::LoadEvent::Committed
                    && let Some(uri) = view.uri()
                {
                    *tab.last_uri.borrow_mut() = uri.to_string();
                    if !browser.hint_shown.get()
                        && browser
                            .config
                            .site_rules
                            .iter()
                            .any(|rule| rule.input_hint && rule.matches(&uri))
                    {
                        browser.hint_shown.set(true);
                        browser.hint_banner.set_revealed(true);
                    }
                }
                browser.sync_toolbar();
            }
        ));
        for property in [
            "title",
            "uri",
            "estimated-load-progress",
            "is-loading",
            "can-go-back",
            "can-go-forward",
        ] {
            view.connect_notify_local(
                Some(property),
                glib::clone!(
                    #[weak]
                    browser,
                    #[weak]
                    tab,
                    move |view, _| {
                        if !browser.is_current_view(&tab, view) {
                            return;
                        }
                        tab.page.set_title(
                            view.title()
                                .as_deref()
                                .filter(|s| !s.is_empty())
                                .unwrap_or("网页"),
                        );
                        tab.page.set_loading(view.is_loading());
                        browser.sync_toolbar();
                    }
                ),
            );
        }
        view.connect_load_failed(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            true,
            move |view, _, uri, error| {
                if error.matches(webkit6::NetworkError::Cancelled)
                    || error.matches(webkit6::PolicyError::FrameLoadInterruptedByPolicyChange)
                {
                    return true;
                }
                if browser.is_current_view(&tab, view) {
                    browser.show_error(&tab, uri, "无法打开网页", "请检查网络连接，或稍后重试。");
                }
                true
            }
        ));
        view.connect_load_failed_with_tls_errors(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            true,
            move |view, uri, _, _| {
                if browser.is_current_view(&tab, view) {
                    browser.show_error(
                        &tab,
                        uri,
                        "无法建立安全连接",
                        "网站证书验证失败。请联系管理员或返回首页。",
                    );
                }
                true
            }
        ));
        view.connect_web_process_terminated(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            move |view, _| {
                if browser.is_current_view(&tab, view) {
                    let uri = tab.last_uri.borrow().clone();
                    browser.show_error(
                        &tab,
                        &uri,
                        "网页意外停止",
                        "可以重新加载此页面，或返回首页。",
                    );
                }
            }
        ));
        view.connect_permission_request(|_, request| {
            request.deny();
            true
        });
        view.connect_run_file_chooser(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            true,
            move |view, request| {
                request.cancel();
                if browser.is_current_view(&tab, view) {
                    browser.notify("此查询终端不支持上传文件。");
                }
                true
            }
        ));
        view.connect_print(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            true,
            move |view, _| {
                if browser.is_current_view(&tab, view) {
                    browser.notify("此查询终端不支持打印。");
                }
                true
            }
        ));
        view.connect_context_menu(|_, menu, _| {
            for item in menu.items() {
                if !matches!(
                    item.stock_action(),
                    webkit6::ContextMenuAction::Copy
                        | webkit6::ContextMenuAction::Cut
                        | webkit6::ContextMenuAction::Paste
                        | webkit6::ContextMenuAction::SelectAll
                        | webkit6::ContextMenuAction::CopyLinkToClipboard
                        | webkit6::ContextMenuAction::OpenLinkInNewWindow
                        | webkit6::ContextMenuAction::GoBack
                        | webkit6::ContextMenuAction::GoForward
                        | webkit6::ContextMenuAction::Reload
                        | webkit6::ContextMenuAction::Stop
                ) {
                    menu.remove(&item);
                }
            }
            false
        });
    }
}

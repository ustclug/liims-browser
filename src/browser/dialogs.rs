use super::*;

impl Browser {
    pub(super) fn message_dialog(title: &str, body: &str) -> adw::AlertDialog {
        let builder =
            gtk::Builder::from_string(include_str!(concat!(env!("OUT_DIR"), "/message.ui")));
        let dialog: adw::AlertDialog = builder.object("dialog").unwrap();
        dialog.set_heading(Some(title));
        dialog.set_body(body);
        dialog
    }

    fn present_for_tab(&self, tab: &Tab, dialog: &adw::AlertDialog) {
        // Prompts belong to their origin tab, and are all closed when it is disposed.
        tab.dialogs
            .borrow_mut()
            .retain(|dialog| dialog.is_visible());
        tab.dialogs.borrow_mut().push(dialog.clone());
        self.tabs.set_selected_page(&tab.page);
        dialog.present(Some(&self.window));
    }

    pub(super) fn connect_web_dialogs(self: &Rc<Self>, tab: &Rc<Tab>, view: &webkit6::WebView) {
        let browser = self;
        view.connect_script_dialog(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            true,
            move |view, request| {
                if !browser.is_current_view(&tab, view) {
                    return true;
                }
                let kind = request.dialog_type();
                if kind == webkit6::ScriptDialogType::BeforeUnloadConfirm {
                    request.confirm_set_confirmed(true);
                    return true;
                }
                let origin = view
                    .uri()
                    .and_then(|uri| url::Url::parse(&uri).ok())
                    .map(|url| url.origin().ascii_serialization())
                    .unwrap_or_else(|| "网页".into());
                let dialog =
                    Self::message_dialog(&origin, request.message().as_deref().unwrap_or(""));
                dialog.add_response("ok", "确定");
                if kind != webkit6::ScriptDialogType::Alert {
                    dialog.add_response("cancel", "取消");
                }
                dialog.set_default_response(Some("ok"));
                dialog.set_close_response(if kind == webkit6::ScriptDialogType::Alert {
                    "ok"
                } else {
                    "cancel"
                });
                let entry = (kind == webkit6::ScriptDialogType::Prompt).then(|| {
                    let entry = gtk::Entry::builder()
                        .text(request.prompt_get_default_text().unwrap_or_default())
                        .build();
                    dialog.set_extra_child(Some(&entry));
                    entry
                });
                let request = request.clone();
                dialog.connect_response(None, move |_, response| {
                    if kind == webkit6::ScriptDialogType::Confirm {
                        request.confirm_set_confirmed(response == "ok");
                    }
                    if response == "ok"
                        && let Some(entry) = &entry
                    {
                        request.prompt_set_text(&entry.text());
                    }
                    request.close();
                });
                browser.present_for_tab(&tab, &dialog);
                true
            }
        ));
        view.connect_authenticate(glib::clone!(
            #[weak]
            browser,
            #[weak]
            tab,
            #[upgrade_or]
            true,
            move |view, request| {
                if !browser.is_current_view(&tab, view) {
                    request.cancel();
                    return true;
                }
                request.set_can_save_credentials(false);
                let dialog = Self::message_dialog(
                    "网站需要登录",
                    &format!(
                        "{}{}",
                        request.host().unwrap_or_default(),
                        if request.is_retry() {
                            "\n登录失败，请检查用户名和密码。"
                        } else {
                            ""
                        }
                    ),
                );
                let builder = gtk::Builder::from_string(include_str!(concat!(
                    env!("OUT_DIR"),
                    "/authentication.ui"
                )));
                let group: adw::PreferencesGroup = builder.object("credentials").unwrap();
                let username: adw::EntryRow = builder.object("username").unwrap();
                let password: adw::PasswordEntryRow = builder.object("password").unwrap();
                dialog.set_extra_child(Some(&group));
                dialog.add_responses(&[("cancel", "取消"), ("login", "登录")]);
                dialog.set_response_appearance("login", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("login"));
                dialog.set_close_response("cancel");
                let request = request.clone();
                dialog.connect_response(None, move |_, response| {
                    if response == "login" {
                        request.authenticate(Some(&webkit6::Credential::new(
                            &username.text(),
                            &password.text(),
                            webkit6::CredentialPersistence::ForSession,
                        )));
                    } else {
                        request.cancel();
                    }
                    password.set_text("");
                });
                browser.present_for_tab(&tab, &dialog);
                true
            }
        ));
    }
}

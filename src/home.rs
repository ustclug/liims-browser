use crate::{config::Profile, navigation};
use adw::prelude::*;
use std::rc::Rc;

pub fn build(
    profile: &Profile,
    navigate: Rc<dyn Fn(String)>,
    activity: Rc<dyn Fn()>,
) -> gtk::Widget {
    let builder = gtk::Builder::from_string(include_str!(concat!(env!("OUT_DIR"), "/home.ui")));
    let home: gtk::ScrolledWindow = builder.object("home").unwrap();
    let campus: gtk::Label = builder.object("campus").unwrap();
    campus.set_text(&profile.name);
    let query: gtk::Entry = builder.object("query").unwrap();
    let search: gtk::Button = builder.object("search").unwrap();
    let template = profile.search_url.clone();
    query.connect_activate({
        let navigate = navigate.clone();
        move |entry| {
            if !entry.text().trim().is_empty() {
                navigate(navigation::search_url(&template, &entry.text()));
            }
        }
    });
    query.connect_changed(move |_| activity());
    search.connect_clicked(move |_| {
        query.emit_activate();
    });
    let links: gtk::FlowBox = builder.object("links").unwrap();
    for link in &profile.links {
        let card =
            gtk::Builder::from_string(include_str!(concat!(env!("OUT_DIR"), "/link-card.ui")));
        let button: gtk::Button = card.object("card").unwrap();
        let icon: gtk::Image = card.object("icon").unwrap();
        let title: gtk::Label = card.object("title").unwrap();
        let description: gtk::Label = card.object("description").unwrap();
        icon.set_icon_name(Some(&link.icon));
        title.set_text(&link.title);
        description.set_text(&link.description);
        let uri = link.url.clone();
        let navigate = navigate.clone();
        button.connect_clicked(move |_| navigate(uri.clone()));
        links.insert(&button, -1);
    }
    let help: gtk::Button = builder.object("help").unwrap();
    help.set_action_name(Some("win.help"));
    home.upcast()
}

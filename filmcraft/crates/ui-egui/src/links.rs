//! Community and project links (Help menu, About dialog, header Discord button, Home screen).
//!
//! FilmCraft comes from the ArtCraft team, whose community Discord and website these link to, and
//! has a public GitHub repository. This modified version doesn't present itself as an ArtCraft
//! product (`docs/brand/LICENSE-brand.txt`): the labels don't use the ArtCraft name, and a build
//! hosted in another app leaves the community and website links out.

/// The app's short name, used in the website and repository URLs.
pub const APP: &str = "filmcraft";

/// The community Discord of FilmCraft's makers.
pub const DISCORD: &str = "https://discord.gg/artcraft";
/// The website of FilmCraft's makers.
pub const WEBSITE: &str = "https://getartcraft.com";
/// FilmCraft's page on that website.
pub const APP_PAGE: &str = "https://getartcraft.com/apps/filmcraft";
/// FilmCraft's source repository.
pub const GITHUB: &str = "https://github.com/storytold/filmcraft";
/// New issue on FilmCraft's repository.
pub const ISSUES: &str = "https://github.com/storytold/filmcraft/issues";

/// (command id, label, url) for every link, in menu order.
pub const ALL: [(&str, &str, &str); 5] = [
    ("help.discord", "Join the Community Discord…", DISCORD),
    ("help.website", "getartcraft.com", WEBSITE),
    ("help.appPage", "FilmCraft Website", APP_PAGE),
    ("help.github", "FilmCraft on GitHub", GITHUB),
    ("help.reportIssue", "Report an Issue…", ISSUES),
];

/// The links to the makers' community and website (their Discord, their website, FilmCraft's page
/// there). A build hosted in another app must not suggest they made or endorse it, and leaves them
/// out (`docs/brand/LICENSE-brand.txt`); FilmCraft's repository and issue tracker stay.
pub const UPSTREAM: [&str; 3] = ["help.discord", "help.website", "help.appPage"];

/// Whether the link command `id` is offered: every link standalone; hosted, not the [`UPSTREAM`] ones.
pub fn offered(id: &str) -> bool {
    !(crate::hosted::is_hosted() && UPSTREAM.contains(&id))
}

/// The URL a `help.*` link command opens (one that is [`offered`]).
pub fn url_for(command: &str) -> Option<&'static str> {
    ALL.iter().find(|(id, _, _)| *id == command && offered(id)).map(|(_, _, u)| *u)
}

/// Open `url` in the system browser (a new tab on the web). The desktop host's
/// [`HostHooks::open_url`](crate::HostHooks::open_url) opens it; without the hook, or when it
/// fails, it goes through `ctx.open_url`, which only does something on the web: the native egui
/// backend is built without its `links` feature, so there it would only log (#642).
pub fn open(app: &mut crate::FilmcraftApp, ctx: &egui::Context, url: &str) {
    if let Some(f) = app.hooks.open_url.as_mut() {
        match f(url) {
            Ok(()) => return,
            Err(e) => log::warn!("can't open {url} in the browser: {e}"),
        }
    }
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_artcraft_scheme() {
        assert_eq!(APP_PAGE, format!("{WEBSITE}/apps/{APP}"));
        assert_eq!(GITHUB, format!("https://github.com/storytold/{APP}"));
        assert!(ALL.iter().all(|(id, _, u)| id.starts_with("help.") && u.starts_with("https://")));
        assert_eq!(url_for("help.discord"), Some(DISCORD));
        assert_eq!(url_for("help.nope"), None);
    }

    #[test]
    fn links_open_through_the_host_hook() {
        // #642: on the desktop every link (Help menu, About, Discord button, Import screen) must
        // reach the host's browser opener; `ctx.open_url` alone is a no-op natively.
        use std::cell::RefCell;
        use std::rc::Rc;
        let opened: Rc<RefCell<Vec<String>>> = Rc::default();
        let rec = opened.clone();
        let mut app = crate::FilmcraftApp::new(filmcraft_engine::Session::default());
        app.hooks.open_url = Some(Box::new(move |u: &str| {
            rec.borrow_mut().push(u.to_string());
            Ok(())
        }));
        let ctx = egui::Context::default();
        for (id, _, url) in ALL {
            let r = crate::menus::invoke(&mut app, &ctx, id, serde_json::json!({})).unwrap();
            assert_eq!(r["url"], url, "{id}");
            assert_eq!(opened.borrow().last().map(String::as_str), Some(url), "{id}");
        }
        open(&mut app, &ctx, DISCORD);
        assert_eq!(opened.borrow().last().map(String::as_str), Some(DISCORD));
        assert_eq!(opened.borrow().len(), ALL.len() + 1);

        // A failing opener falls back to `ctx.open_url` instead of losing the click.
        app.hooks.open_url = Some(Box::new(|_: &str| Err("no browser".into())));
        open(&mut app, &ctx, ISSUES);
        let fallback: Vec<String> =
            ctx.output(|o| o.commands.iter().filter_map(|c| if let egui::OutputCommand::OpenUrl(u) = c { Some(u.url.clone()) } else { None }).collect());
        assert_eq!(fallback, [ISSUES]);
    }
}

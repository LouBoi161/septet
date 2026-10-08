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

/// Open `url` in the system browser (a new tab on the web).
pub fn open(ctx: &egui::Context, url: &str) {
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
}

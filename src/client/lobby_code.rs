//! wash-fps `client/LobbyCode.ts`: room codes, a room named without a URL,
//! for clients whose address is not a link anyone could follow (a hosted
//! build, this native client).
//!
//! - `eu:3fa9c2d1`: a public room in a region.
//! - `eu:3fa9c2d1:<invite>`: a private room, which also needs its invite.
//! - `3fa9c2d1` (or `3fa9c2d1:<invite>`): a room in this client's region.
//!
//! A room exists only on its region's lobby, so a code carries the region.
//! The parser also takes a room link from any region's site.

use crate::regions::Region;

/// `ROOM_ID` and `INVITE`: letters, digits, `_` and `-`, up to 64 and 128.
fn room_id_ok(id: &str) -> bool {
    word_ok(id, 64)
}

fn invite_ok(invite: &str) -> bool {
    word_ok(invite, 128)
}

fn word_ok(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `RoomRef`: a room, the region it is in (`None` for this client's own)
/// and a private room's invite.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomRef {
    pub region: Option<Region>,
    pub lobby_id: String,
    pub invite: Option<String>,
}

/// `CodeTarget`: what a pasted text names.
#[derive(Debug, Clone, PartialEq)]
pub enum CodeTarget {
    Room(RoomRef),
    /// A link to a site that is not a Washoi region.
    Elsewhere(String),
}

/// `roomCode`: the code for a room in `region` (this client's).
#[must_use]
pub fn room_code(lobby_id: &str, invite: Option<&str>, region: Option<&Region>) -> String {
    [region.map(|r| r.id.as_str()), Some(lobby_id), invite]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

/// The host of `url` (`https://host[:port]/…`), without its port.
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    Some(host.split(':').next().unwrap_or(host)).filter(|h| !h.is_empty())
}

/// `host[:port]` of `url`, as `URL.host` reads it.
fn authority_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    rest.split(['/', '?', '#']).next().filter(|a| !a.is_empty())
}

/// `fromLink`: `?lobby=` (or `/lobby/<id>`) and `?invite=` on a region's site.
fn from_link(url: &str, regions: &[Region], page_host: Option<&str>) -> Option<CodeTarget> {
    let host = host_of(url)?;
    let after = url.split_once("://")?.1;
    let path_and_query = &after[authority_of(url)?.len()..];
    let without_fragment = path_and_query.split('#').next().unwrap_or("");
    let (path, query) = without_fragment.split_once('?').unwrap_or((without_fragment, ""));
    let param = |name: &str| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_owned())
    };
    let from_path = || {
        let id = path.strip_prefix("/lobby/")?.trim_end_matches('/');
        Some(id.to_owned()).filter(|id| !id.contains('/'))
    };
    let lobby_id = param("lobby").or_else(from_path)?;
    if !room_id_ok(&lobby_id) {
        return None;
    }
    let invite = param("invite").filter(|i| invite_ok(i));
    let region = regions
        .iter()
        .find(|r| r.host == host || r.data_url.as_deref().and_then(host_of).is_some_and(|h| h == host));
    if let Some(region) = region {
        return Some(CodeTarget::Room(RoomRef {
            region: Some(region.clone()),
            lobby_id,
            invite,
        }));
    }
    // A link to the site this client is on is this client's region.
    let authority = authority_of(url)?;
    if page_host == Some(authority) {
        return Some(CodeTarget::Room(RoomRef {
            region: None,
            lobby_id,
            invite,
        }));
    }
    Some(CodeTarget::Elsewhere(authority.to_owned()))
}

/// `parseRoomCode`: the room a pasted text names, or that it is a link to
/// another site, or `None`. `page_host` is this client's own site, whose
/// room links are this region's.
#[must_use]
pub fn parse_room_code(input: &str, regions: &[Region], page_host: Option<&str>) -> Option<CodeTarget> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return from_link(text, regions, page_host);
    }
    let parts: Vec<&str> = text.split(':').collect();
    let region = regions.iter().find(|r| r.id == parts[0].to_ascii_lowercase());
    let rest = if region.is_some() { &parts[1..] } else { &parts[..] };
    let (lobby_id, invite) = match rest {
        [id] => (*id, None),
        [id, invite] => (*id, Some(*invite)),
        _ => return None,
    };
    if !room_id_ok(lobby_id) || invite.is_some_and(|i| !invite_ok(i)) {
        return None;
    }
    Some(CodeTarget::Room(RoomRef {
        region: region.cloned(),
        lobby_id: lobby_id.to_owned(),
        invite: invite.map(str::to_owned),
    }))
}

/// `roomInText`: the first room a chat message names, a room link or a code
/// with its region (`eu:3fa9c2d1`). A bare id is too much like an ordinary
/// word to count.
#[must_use]
pub fn room_in_text(text: &str, regions: &[Region]) -> Option<RoomRef> {
    for word in text.split_whitespace() {
        let candidate = word.trim_end_matches([')', ',', '.', '!', '?']);
        let lower = candidate.to_ascii_lowercase();
        let is_link = lower.starts_with("http://") || lower.starts_with("https://");
        let region_code = regions.iter().any(|r| lower.starts_with(&format!("{}:", r.id)));
        if !is_link && !region_code {
            continue;
        }
        if let Some(CodeTarget::Room(room)) = parse_room_code(candidate, regions, None) {
            return Some(room);
        }
    }
    None
}

/// `inThisRegion`: whether `room` is in the region `here` plays in.
#[must_use]
pub fn in_this_region(room: &RoomRef, here: Option<&Region>) -> bool {
    match (&room.region, here) {
        (None, _) => true,
        (Some(r), Some(h)) => r.id == h.id,
        (Some(_), None) => false,
    }
}

/// The Join lobby dialog's line about what the field's text means
/// (`describe`).
#[must_use]
pub fn describe(target: Option<&CodeTarget>, here: Option<&Region>) -> String {
    match target {
        None => "Paste a room link, or a code like eu:3fa9c2d1.".to_owned(),
        Some(CodeTarget::Elsewhere(host)) => format!("That link is for {host}, not a Washoi region."),
        Some(CodeTarget::Room(room)) if in_this_region(room, here) => format!(
            "Room {}{}.",
            room.lobby_id,
            if room.invite.is_some() { " (private)" } else { "" }
        ),
        Some(CodeTarget::Room(room)) => format!(
            "That room is in {}, where you play as a separate player. Joining switches region.",
            room.region.as_ref().map_or("another region", |r| r.name.as_str())
        ),
    }
}

/// A room link on `region`'s site, which `--join` reads: the room handed to
/// the game when it starts again in that region.
#[must_use]
pub fn region_room_link(room: &RoomRef) -> String {
    let host = room.region.as_ref().map_or("", |r| r.host.as_str());
    match &room.invite {
        Some(invite) => format!("https://{host}/?lobby={}&invite={invite}", room.lobby_id),
        None => format!("https://{host}/?lobby={}", room.lobby_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regions::builtin;

    fn eu_us() -> (Region, Region) {
        let r = builtin();
        (r[0].clone(), r[1].clone())
    }

    fn room(region: Option<&Region>, id: &str, invite: Option<&str>) -> Option<CodeTarget> {
        Some(CodeTarget::Room(RoomRef {
            region: region.cloned(),
            lobby_id: id.to_owned(),
            invite: invite.map(str::to_owned),
        }))
    }

    #[test]
    fn codes_write_the_region_the_room_and_a_private_room_s_invite() {
        let (eu, us) = eu_us();
        let invite = "q".repeat(32);
        assert_eq!(room_code("3fa9c2d1", None, Some(&eu)), "eu:3fa9c2d1");
        assert_eq!(
            room_code("3fa9c2d1", Some(&invite), Some(&us)),
            format!("us:3fa9c2d1:{invite}")
        );
        assert_eq!(room_code("3fa9c2d1", None, None), "3fa9c2d1");
    }

    #[test]
    fn codes_read_with_a_region_without_one_and_with_an_invite() {
        let regions = builtin();
        let (eu, us) = eu_us();
        let invite = "q".repeat(32);
        assert_eq!(
            parse_room_code("eu:3fa9c2d1", &regions, None),
            room(Some(&eu), "3fa9c2d1", None)
        );
        assert_eq!(
            parse_room_code(&format!(" US:3fa9c2d1:{invite} "), &regions, None),
            room(Some(&us), "3fa9c2d1", Some(&invite))
        );
        assert_eq!(
            parse_room_code("3fa9c2d1", &regions, None),
            room(None, "3fa9c2d1", None)
        );
    }

    #[test]
    fn room_links_from_a_region_s_site_and_another_site_told_apart() {
        let regions = builtin();
        let (eu, us) = eu_us();
        let invite = "q".repeat(32);
        assert_eq!(
            parse_room_code(
                &format!("https://us.washoi.net/?lobby=3fa9c2d1&invite={invite}"),
                &regions,
                None
            ),
            room(Some(&us), "3fa9c2d1", Some(&invite))
        );
        assert_eq!(
            parse_room_code("https://eu.washoi.net/lobby/3fa9c2d1", &regions, None),
            room(Some(&eu), "3fa9c2d1", None)
        );
        assert_eq!(
            parse_room_code("https://example.com/?lobby=3fa9c2d1", &regions, Some("eu.washoi.net")),
            Some(CodeTarget::Elsewhere("example.com".into()))
        );
        assert_eq!(
            parse_room_code("http://localhost:3000/?lobby=r1", &regions, Some("localhost:3000")),
            room(None, "r1", None)
        );
    }

    #[test]
    fn anything_else_is_refused() {
        let regions = builtin();
        for text in [
            "",
            "eu:",
            "eu:a:b:c",
            "hello world",
            "eu:<script>",
            "https://eu.washoi.net/",
        ] {
            assert_eq!(parse_room_code(text, &regions, None), None, "{text}");
        }
    }

    #[test]
    fn a_chat_message_names_a_room_only_by_a_link_or_a_code_with_its_region() {
        let regions = builtin();
        let (_, us) = eu_us();
        assert_eq!(
            room_in_text("join me: eu:3fa9c2d1!", &regions).map(|r| r.lobby_id),
            Some("3fa9c2d1".into())
        );
        assert_eq!(
            room_in_text("go https://us.washoi.net/?lobby=3fa9c2d1.", &regions).and_then(|r| r.region),
            Some(us)
        );
        assert_eq!(room_in_text("3fa9c2d1 is a nice number", &regions), None);
        assert_eq!(room_in_text("ratio 3:1", &regions), None);
    }

    #[test]
    fn rooms_in_this_client_s_region() {
        let (eu, us) = eu_us();
        let at = |region: Option<&Region>| RoomRef {
            region: region.cloned(),
            lobby_id: "a".into(),
            invite: None,
        };
        assert!(in_this_region(&at(None), Some(&eu)));
        assert!(in_this_region(&at(Some(&eu)), Some(&eu)));
        assert!(!in_this_region(&at(Some(&us)), Some(&eu)));
    }

    #[test]
    fn the_dialog_says_what_the_text_names() {
        let regions = builtin();
        let (eu, _) = eu_us();
        let invite = "q".repeat(32);
        let said = |text: &str| describe(parse_room_code(text, &regions, None).as_ref(), Some(&eu));
        assert_eq!(said(&format!("eu:3fa9c2d1:{invite}")), "Room 3fa9c2d1 (private).");
        assert!(said("us:3fa9c2d1").starts_with("That room is in North America"));
        assert_eq!(said("not a room"), "Paste a room link, or a code like eu:3fa9c2d1.");
    }

    #[test]
    fn a_room_in_another_region_is_joined_by_its_link_there() {
        let (_, us) = eu_us();
        let r = RoomRef {
            region: Some(us),
            lobby_id: "3fa9c2d1".into(),
            invite: Some("tok".into()),
        };
        assert_eq!(region_room_link(&r), "https://us.washoi.net/?lobby=3fa9c2d1&invite=tok");
    }
}

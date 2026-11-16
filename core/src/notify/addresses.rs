//! What the user chose for mail that came to one of their own addresses.
//!
//! Each address of the account (its identities) can notify (the default), notify without sound
//! ("silent": shown, in a quiet channel) or not notify at all. The choice is kept on the device
//! only. Which address a message came to is decided from the cache alone (a notification must not
//! cost a network call): its To list, its Cc list and the delivery headers the server wrote
//! (`Delivered-To`, which is also where a Bcc shows), whichever of them the cache already holds. An
//! address of the message that is one of the user's own counts, exactly or with a plus tag
//! (`me+shop@x.org` for `me@x.org`). A message to several of the user's addresses takes the
//! loudest choice (the first of them when equally loud); a message that matches none takes the default.

use std::collections::{HashMap, HashSet};

use crate::{
    compose::rules::{address_key, own_keys, plus_base},
    model::{Address, Identity},
};

/// What happens to mail that came to an address. The order is the loudness: a message to
/// several addresses takes the greatest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, uniffi::Enum)]
pub enum AddressMode {
    /// Not shown at all: such mail is no part of what the notification rule keeps on screen (and
    /// is not counted as swiped).
    Off,
    /// Shown without sound or vibration.
    Silent,
    /// Shown with the account's sound. The default.
    Notify,
}

impl AddressMode {
    /// What the cache stores: `Notify` is the absence of a row.
    pub(crate) fn code(self) -> i64 {
        match self {
            AddressMode::Off => 0,
            AddressMode::Silent => 1,
            AddressMode::Notify => 2,
        }
    }

    pub(crate) fn from_code(code: i64) -> Self {
        match code {
            0 => AddressMode::Off,
            1 => AddressMode::Silent,
            _ => AddressMode::Notify,
        }
    }
}

/// One address of the account and its choice, for the settings screen.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AddressChoice {
    pub name: String,
    pub address: String,
    pub mode: AddressMode,
}

/// The key a choice is stored under: the address in lower case.
pub fn choice_key(address: &str) -> String {
    address_key(address)
}

/// The choices as the settings screen lists them: every identity once (in the server's order),
/// with its mode, `Notify` where nothing was chosen.
pub fn choices(identities: &[Identity], chosen: &HashMap<String, AddressMode>) -> Vec<AddressChoice> {
    let mut seen = HashSet::new();
    identities
        .iter()
        .filter(|i| seen.insert(choice_key(&i.email)))
        .map(|i| AddressChoice {
            name: i.name.clone(),
            address: i.email.clone(),
            mode: chosen.get(&choice_key(&i.email)).copied().unwrap_or(AddressMode::Notify),
        })
        .collect()
}

/// The own address (as its key) that decides what happens to a message sent to these addresses, with
/// its mode: the loudest, the first of them when equally loud. `None` when no address is the user's own.
/// `own` are the keys of the user's addresses ([`own_keys`]), `chosen` the stored choices by [`choice_key`].
pub fn receiving(
    to: &[Address],
    own: &HashSet<String>,
    chosen: &HashMap<String, AddressMode>,
) -> Option<(String, AddressMode)> {
    let mut best: Option<(String, AddressMode)> = None;
    for a in to {
        let exact = address_key(&a.email);
        let key =
            if own.contains(&exact) { Some(exact) } else { plus_base(&a.email).filter(|base| own.contains(base)) };
        let Some(key) = key else { continue };
        let mode = chosen.get(&key).copied().unwrap_or(AddressMode::Notify);
        if best.as_ref().is_none_or(|(_, m)| mode > *m) {
            best = Some((key, mode));
        }
    }
    best
}

/// The mode of a message that was sent `to` these addresses; the default when none is the user's own.
#[cfg(test)]
fn mode_for(to: &[Address], own: &HashSet<String>, chosen: &HashMap<String, AddressMode>) -> AddressMode {
    receiving(to, own, chosen).map_or(AddressMode::Notify, |(_, mode)| mode)
}

/// Where one mail notification goes: the own address it came to (its key; `None`: the account's default
/// channel) and whether it is shown quietly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Route {
    pub address: Option<String>,
    pub quiet: bool,
}

impl Route {
    /// What is stored with a shown notification, to tell that it is in another channel than it should be.
    pub fn code(&self) -> String {
        match (&self.address, self.quiet) {
            (None, false) => String::new(),
            (None, true) => "q".to_string(),
            (Some(a), false) => format!("n:{a}"),
            (Some(a), true) => format!("q:{a}"),
        }
    }
}

/// Splits `desired` by the choices: what is left to show, and the route of each of those. Mail to an
/// address that is off is dropped.
pub fn apply<T>(
    desired: Vec<T>,
    to_of: impl Fn(&T) -> Vec<Address>,
    id_of: impl Fn(&T) -> &str,
    identities: &[Identity],
    chosen: &HashMap<String, AddressMode>,
) -> (Vec<T>, HashMap<String, Route>) {
    let own = own_keys(identities);
    let mut routes = HashMap::new();
    let kept = desired
        .into_iter()
        .filter(|m| {
            let Some((address, mode)) = receiving(&to_of(m), &own, chosen) else { return true };
            match mode {
                AddressMode::Off => false,
                AddressMode::Silent => {
                    routes.insert(id_of(m).to_string(), Route { address: Some(address), quiet: true });
                    true
                }
                AddressMode::Notify => {
                    routes.insert(id_of(m).to_string(), Route { address: Some(address), quiet: false });
                    true
                }
            }
        })
        .collect();
    (kept, routes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(email: &str) -> Address {
        Address { name: String::new(), email: email.into() }
    }

    fn identity(email: &str) -> Identity {
        Identity { id: email.into(), name: "Me".into(), email: email.into(), signature: String::new() }
    }

    fn chosen(list: &[(&str, AddressMode)]) -> HashMap<String, AddressMode> {
        list.iter().map(|(a, m)| (choice_key(a), *m)).collect()
    }

    fn own() -> HashSet<String> {
        own_keys(&[identity("me@x.org"), identity("Work@x.org"), identity("shop@x.org")])
    }

    #[test]
    fn the_loudness_runs_from_off_to_notify() {
        assert!(AddressMode::Off < AddressMode::Silent && AddressMode::Silent < AddressMode::Notify);
        for m in [AddressMode::Off, AddressMode::Silent] {
            assert_eq!(AddressMode::from_code(m.code()), m);
        }
    }

    #[test]
    fn mail_to_an_address_takes_its_choice_and_unknown_addresses_notify() {
        let c = chosen(&[("me@x.org", AddressMode::Silent), ("work@x.org", AddressMode::Off)]);
        let mode = |to: &[&str]| mode_for(&to.iter().map(|e| addr(e)).collect::<Vec<_>>(), &own(), &c);
        assert_eq!(mode(&["ME@x.org"]), AddressMode::Silent);
        assert_eq!(mode(&["work@X.org"]), AddressMode::Off);
        assert_eq!(mode(&["shop@x.org"]), AddressMode::Notify, "no choice: the default");
        assert_eq!(mode(&["list@other.org"]), AddressMode::Notify, "not one of mine");
        assert_eq!(mode(&[]), AddressMode::Notify, "Bcc or a list: nothing to go by");
    }

    #[test]
    fn a_plus_tag_belongs_to_its_address() {
        let c = chosen(&[("me@x.org", AddressMode::Off)]);
        assert_eq!(mode_for(&[addr("me+news@x.org")], &own(), &c), AddressMode::Off);
        assert_eq!(mode_for(&[addr("you+news@x.org")], &own(), &c), AddressMode::Notify);
        // An identity that is itself tagged is matched exactly first.
        let tagged = own_keys(&[identity("me@x.org"), identity("me+news@x.org")]);
        let c = chosen(&[("me@x.org", AddressMode::Off), ("me+news@x.org", AddressMode::Silent)]);
        assert_eq!(mode_for(&[addr("me+news@x.org")], &tagged, &c), AddressMode::Silent);
    }

    #[test]
    fn mail_to_several_of_my_addresses_takes_the_loudest_choice() {
        let c = chosen(&[("me@x.org", AddressMode::Off), ("work@x.org", AddressMode::Silent)]);
        assert_eq!(mode_for(&[addr("me@x.org"), addr("work@x.org")], &own(), &c), AddressMode::Silent);
        // One more of mine with no choice is a notify.
        assert_eq!(mode_for(&[addr("me@x.org"), addr("shop@x.org")], &own(), &c), AddressMode::Notify);
        // Someone else's address on the message does not count.
        assert_eq!(mode_for(&[addr("me@x.org"), addr("bo@y.org")], &own(), &c), AddressMode::Off);
    }

    #[test]
    fn apply_drops_the_off_mail_and_routes_the_rest() {
        let ids = [identity("me@x.org"), identity("work@x.org")];
        let c = chosen(&[("me@x.org", AddressMode::Off), ("work@x.org", AddressMode::Silent)]);
        let mail = vec![("a", vec![addr("me@x.org")]), ("b", vec![addr("Work+x@x.org")]), ("c", vec![addr("z@y.org")])];
        let (kept, routes) = apply(mail, |m| m.1.clone(), |m| m.0, &ids, &c);
        assert_eq!(kept.iter().map(|m| m.0).collect::<Vec<_>>(), ["b", "c"]);
        assert_eq!(routes.get("b"), Some(&Route { address: Some("work@x.org".into()), quiet: true }));
        assert_eq!(routes.get("c"), None, "not one of mine: the default channel");
        // Nothing chosen: mail to one of my addresses still has its address.
        let (kept, routes) =
            apply(vec![("a", vec![addr("ME@x.org")])], |m| m.1.clone(), |m| m.0, &ids, &HashMap::new());
        assert_eq!(kept.len(), 1);
        assert_eq!(routes["a"], Route { address: Some("me@x.org".into()), quiet: false });
    }

    #[test]
    fn the_loudest_address_gives_the_route_and_the_first_wins_a_tie() {
        let c = chosen(&[("me@x.org", AddressMode::Silent)]);
        let both = [addr("me@x.org"), addr("work@x.org"), addr("shop@x.org")];
        assert_eq!(receiving(&both, &own(), &c), Some(("work@x.org".into(), AddressMode::Notify)));
        assert_eq!(receiving(&[addr("a@b.org")], &own(), &c), None);
    }

    #[test]
    fn a_route_has_a_code_that_tells_one_from_the_other() {
        let codes: HashSet<String> = [
            Route::default(),
            Route { address: None, quiet: true },
            Route { address: Some("me@x.org".into()), quiet: false },
            Route { address: Some("me@x.org".into()), quiet: true },
            Route { address: Some("work@x.org".into()), quiet: false },
        ]
        .iter()
        .map(Route::code)
        .collect();
        assert_eq!(codes.len(), 5);
        assert_eq!(Route::default().code(), "");
    }

    #[test]
    fn the_settings_list_has_every_identity_once() {
        let ids = [identity("me@x.org"), identity("ME@x.org"), identity("work@x.org")];
        let list = choices(&ids, &chosen(&[("work@x.org", AddressMode::Silent)]));
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].mode, list[1].mode), (AddressMode::Notify, AddressMode::Silent));
    }
}

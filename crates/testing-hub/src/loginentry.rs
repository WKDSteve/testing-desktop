//! The start of the hub with the login, as the page sees it: its state, on and off. Nothing registers it by itself;
//! the person does, with the switch of the page (Help menu), with `testing-hub autostart on|off` or with the switch
//! of its tile in hub-desktop: all three read and switch the one state of the kit (`appkit_hub::loginstate`: the
//! hub's own login entry, or `start_with_hub` of hub-desktop while hub-desktop starts with the login). An entry that
//! is there stays until they take it away (an update does not touch it). Its own entry names the installed copy,
//! whichever copy of the program is running. Only the PC asks: the route is not one a phone may reach.

use appkit_hub::env::Env;
use appkit_hub::loginstate::{self, SystemLogin};

/// The start of this hub with the login. The program has [`SystemLoginEntry`]; a test has one that holds a value.
pub trait LoginEntry: Send + Sync {
    /// Whether it is on; `None`: this system cannot tell, and the page offers no switch.
    fn is_on(&self) -> Option<bool>;

    /// Switch it on or off.
    ///
    /// # Errors
    ///
    /// What the system said.
    fn set(&self, on: bool) -> Result<(), String>;
}

/// The real one: the kit's one state on this machine.
pub struct SystemLoginEntry;

impl LoginEntry for SystemLoginEntry {
    fn is_on(&self) -> Option<bool> {
        loginstate::is_on(&crate::IDENTITY, &Env::from_process(), &SystemLogin)
    }

    fn set(&self, on: bool) -> Result<(), String> {
        let env = Env::from_process();
        loginstate::set(
            &crate::IDENTITY,
            on,
            || loginstate::installed_program(&crate::IDENTITY, &env),
            &env,
            &SystemLogin,
        )
        .map(|_| ())
    }
}

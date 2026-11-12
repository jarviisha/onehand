//! Where the settings' drive meets the tools': outside control's value is the
//! tools' drive's, which the socket reads on every message, and its save and
//! put-back are the settings' drive's. What the settings put in force for it
//! goes through here.

use super::drive::ControlDrive;
use crate::config::drive::Value;

/// Put `value`, shown by the settings' drive, in force on the tools.
pub fn shown(control: &mut ControlDrive, value: &Value) {
    match value {
        Value::Outside(on) => {
            control.set_outside(*on);
        }
        Value::Appearance(_) | Value::Agents | Value::Workflows { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::drive::{Effect, Input, SettingsDrive};
    use crate::control::{Peer, Standing};

    /// Outside control switched on is in force at once; a write that fails
    /// puts it back off for the socket too.
    #[test]
    fn outside_control_follows_its_save() {
        let mut settings = SettingsDrive::new(Vec::new(), Default::default());
        let mut control = ControlDrive::default();
        let mut write = None;
        for effect in settings.set_outside(control.outside(), true) {
            match effect {
                Effect::Show(value) => {
                    shown(&mut control, &value);
                }
                Effect::Write { pending, edit } => write = Some((pending, edit)),
                Effect::Probe { .. }
                | Effect::Look { .. }
                | Effect::CallOff { .. }
                | Effect::Changed
                | Effect::Signal(_) => {}
            }
        }
        assert_eq!(standing(&control), "On");
        let (pending, edit) = write.expect("a write");
        let fx = settings.handle(Input::Written {
            pending,
            edit,
            result: Err("read-only".into()),
        });
        for effect in fx {
            match effect {
                Effect::Show(value) => shown(&mut control, &value),
                Effect::Write { .. }
                | Effect::Probe { .. }
                | Effect::Look { .. }
                | Effect::CallOff { .. }
                | Effect::Changed
                | Effect::Signal(_) => {}
            }
        }
        assert_eq!(standing(&control), "Refused");
    }

    fn standing(control: &ControlDrive) -> &'static str {
        match control.standing(&Peer::Outside) {
            Standing::On => "On",
            Standing::Off => "Off",
            Standing::Only(_) => "Only",
            Standing::Refused(_) => "Refused",
        }
    }
}

//! Rebind framework actions while retaining their original focus predicates and
//! payloads. App commands use the simpler `[keybindings]` table instead.
use gpui_kit::{
    DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate, KeyBindingMetaIndex, Keystroke,
};
use serde::Deserialize;

/// Tags bindings owned by this layer so reload can recover pristine defaults.
pub const OVERRIDE_META: KeyBindingMetaIndex = KeyBindingMetaIndex(u32::MAX);

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub action: String,
    pub keys: Vec<String>,
    /// Omitted matches every original context; an empty string means global.
    pub context: Option<String>,
    /// Optional original shortcut, including an entire multi-stroke chord.
    pub from: Option<String>,
}

pub fn strokes(value: &str) -> Result<Vec<Keystroke>, String> {
    if value.trim().is_empty() {
        return Err("Empty shortcut; use keys = [] to unbind".into());
    }
    value
        .split_whitespace()
        .map(|stroke| {
            Keystroke::parse(stroke).map_err(|error| format!("Invalid shortcut {value:?}: {error}"))
        })
        .collect()
}
fn predicate(value: &str) -> Result<Option<String>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    KeyBindingContextPredicate::parse(value)
        .map(|context| Some(context.to_string()))
        .map_err(|error| format!("Invalid keymap context {value:?}: {error}"))
}
pub fn validate(overrides: &[Override]) -> Result<(), String> {
    for rule in overrides {
        if rule.action.trim().is_empty() {
            return Err("keymap.action must name a framework action".into());
        }
        if rule.action.starts_with("jaywalk::") {
            return Err("Use [keybindings] to rebind Jaywalk commands".into());
        }
        for key in &rule.keys {
            strokes(key)?;
        }
        if let Some(from) = &rule.from {
            strokes(from)?;
        }
        if let Some(context) = &rule.context {
            predicate(context)?;
        }
    }
    Ok(())
}
fn context(binding: &KeyBinding) -> Option<String> {
    binding.predicate().map(|p| p.to_string())
}

/// Semantic identity excludes metadata; action equality includes its parameters.
pub fn same_binding(a: &KeyBinding, b: &KeyBinding) -> bool {
    a.keystrokes() == b.keystrokes()
        && context(a) == context(b)
        && a.action().partial_eq(b.action())
}

pub fn remap(defaults: &[KeyBinding], overrides: &[Override]) -> Result<Vec<KeyBinding>, String> {
    validate(overrides)?;
    let mut matched = vec![false; overrides.len()];
    let mut result: Vec<KeyBinding> = Vec::new();
    for binding in defaults {
        let mut matching = None;
        for (index, rule) in overrides.iter().enumerate() {
            if binding.action().name() != rule.action {
                continue;
            }
            if let Some(selector) = &rule.context
                && predicate(selector)? != context(binding)
            {
                continue;
            }
            if let Some(from) = &rule.from
                && binding.match_keystrokes(&strokes(from)?) != Some(false)
            {
                continue;
            }
            if matching.is_some() {
                return Err(format!(
                    "Overlapping keymap overrides for {}; use context/from to select distinct defaults",
                    rule.action
                ));
            }
            matching = Some(index);
        }
        let Some(index) = matching else {
            result.push(binding.clone());
            continue;
        };
        matched[index] = true;
        for key in overrides[index].keys.iter().rev() {
            let replacement = KeyBinding::load(
                key,
                binding.action().boxed_clone(),
                binding.predicate(),
                false,
                binding.action_input(),
                &DummyKeyboardMapper,
            )
            .map_err(|error| format!("Invalid keymap shortcut {key:?}: {error}"))?
            .with_meta(OVERRIDE_META);
            if !result.iter().any(|old| same_binding(old, &replacement)) {
                result.push(replacement);
            }
        }
    }
    for (index, found) in matched.into_iter().enumerate() {
        if !found {
            return Err(format!(
                "No default binding matches keymap action {:?}, context {:?}, from {:?}. Check the framework action name and original context/shortcut",
                overrides[index].action, overrides[index].context, overrides[index].from
            ));
        }
    }
    for (index, binding) in result.iter().enumerate() {
        for other in &result[..index] {
            if (binding.meta() == Some(OVERRIDE_META) || other.meta() == Some(OVERRIDE_META))
                && binding.keystrokes() == other.keystrokes()
                && context(binding) == context(other)
                && !binding.action().partial_eq(other.action())
            {
                return Err(format!(
                    "Keymap shortcut conflicts between {} and {} in context {:?}; unbind the other action or narrow the override with from",
                    binding.action().name(),
                    other.action().name(),
                    context(binding)
                ));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::base::input::{Copy, SelectAll};
    use gpui_kit::{Action, KeyContext, Keymap};
    fn rule(action: &str, keys: &[&str]) -> Override {
        Override {
            action: action.into(),
            keys: keys.iter().map(|key| (*key).into()).collect(),
            context: None,
            from: None,
        }
    }
    #[test]
    fn copy_remapping_and_unbinding_keep_original_contexts_and_restore_defaults() {
        assert_eq!(Copy.name(), "input::Copy");
        let defaults = vec![
            KeyBinding::new("secondary-c", Copy, Some("Input")),
            KeyBinding::new("secondary-c", Copy, Some("Diff")),
        ];
        let remapped = remap(&defaults, &[rule("input::Copy", &["secondary-insert"])]).unwrap();
        let map = Keymap::new(remapped);
        for context in ["Input", "Diff"] {
            let contexts = [KeyContext::parse(context).unwrap()];
            assert!(
                map.bindings_for_input(&strokes("secondary-c").unwrap(), &contexts)
                    .0
                    .is_empty()
            );
            assert_eq!(
                map.bindings_for_input(&strokes("secondary-insert").unwrap(), &contexts)
                    .0
                    .len(),
                1
            );
        }
        assert!(
            remap(&defaults, &[rule("input::Copy", &[])])
                .unwrap()
                .is_empty()
        );
        assert_eq!(remap(&defaults, &[]).unwrap().len(), 2);
        assert_eq!(
            defaults[0].keystrokes(),
            KeyBinding::new("secondary-c", Copy, Some("Input")).keystrokes()
        );
    }
    #[test]
    fn context_and_original_shortcut_select_parameterized_actions_without_changing_payloads() {
        use gpui_kit::base::actions::Confirm;
        let defaults = vec![
            KeyBinding::new("enter", Confirm { secondary: false }, Some("Command")),
            KeyBinding::new(
                "secondary-enter",
                Confirm { secondary: true },
                Some("Command"),
            ),
        ];
        let mut override_ = rule("ui::Confirm", &["space"]);
        override_.context = Some("Command".into());
        override_.from = Some("enter".into());
        let result = remap(&defaults, &[override_]).unwrap();
        assert!(
            !result[0]
                .action()
                .as_any()
                .downcast_ref::<Confirm>()
                .unwrap()
                .secondary
        );
        assert!(
            result[1]
                .action()
                .as_any()
                .downcast_ref::<Confirm>()
                .unwrap()
                .secondary
        );
    }
    #[test]
    fn invalid_overlapping_unknown_and_conflicting_rules_are_errors() {
        let defaults = vec![
            KeyBinding::new("secondary-c", Copy, Some("Input")),
            KeyBinding::new("secondary-a", SelectAll, Some("Input")),
        ];
        assert!(remap(&defaults, &[rule("input::Typo", &[])]).is_err());
        assert!(remap(&defaults, &[rule("input::Copy", &["secondary-a"])]).is_err());
        assert!(
            remap(
                &defaults,
                &[
                    rule("input::Copy", &["secondary-a"]),
                    rule("input::SelectAll", &[])
                ]
            )
            .is_ok()
        );
        assert!(
            remap(
                &defaults,
                &[rule("input::Copy", &[]), rule("input::Copy", &[])]
            )
            .is_err()
        );
        let mut invalid = rule("input::Copy", &["mystery-c"]);
        assert!(validate(&[invalid.clone()]).is_err());
        invalid.keys.clear();
        invalid.context = Some("Input &&".into());
        assert!(validate(&[invalid]).is_err());
    }
}

use crate::app::Mode;
use crate::command::{
    CommandInvocationSource, CommandPolicyContext, command_registry, find_command_spec,
};
use crate::condition::RuntimeConditionContext;
use crate::extension::ExtensionUiSnapshot;
use crate::input::keymap::build_builtin_sequence_registry;
use crate::input::sequence::KeyBindingScope;
use crate::palette::PaletteKind;

use super::spec::validate_command_id_for_source;

#[test]
fn builtin_keymap_references_registered_keymap_invocable_commands() {
    let registry = build_builtin_sequence_registry();
    let snapshot = registry.snapshot();
    let extensions = ExtensionUiSnapshot::with_search_active(true);

    assert!(
        !snapshot.exact_bindings.is_empty(),
        "builtin keymap must define at least one exact binding"
    );
    assert!(
        !snapshot.numeric_prefix_bindings.is_empty(),
        "builtin keymap must define at least one numeric prefix binding"
    );

    for binding in snapshot.exact_bindings {
        let ctx = key_binding_command_context(binding.scope, &extensions);
        assert!(
            find_command_spec(binding.command_id).is_some(),
            "key binding {:?} references unknown command {}",
            binding.keys,
            binding.command_id
        );
        validate_command_id_for_source(binding.command_id, &ctx).unwrap_or_else(|err| {
            panic!(
                "key binding {:?} references command {} that keymap cannot invoke: {}",
                binding.keys, binding.command_id, err
            )
        });
    }

    for binding in snapshot.numeric_prefix_bindings {
        let ctx = key_binding_command_context(binding.scope, &extensions);
        assert!(
            find_command_spec(binding.command_id).is_some(),
            "numeric key binding {:?} references unknown command {}",
            binding.suffix,
            binding.command_id
        );
        validate_command_id_for_source(binding.command_id, &ctx).unwrap_or_else(|err| {
            panic!(
                "numeric key binding {:?} references command {} that keymap cannot invoke: {}",
                binding.suffix, binding.command_id, err
            )
        });
    }

    for binding in snapshot.generated_bindings {
        let ctx = key_binding_command_context(binding.scope, &extensions);
        assert!(
            find_command_spec(binding.command_id).is_some(),
            "generated key binding {:?} references unknown command {}",
            binding.matcher,
            binding.command_id
        );
        validate_command_id_for_source(binding.command_id, &ctx).unwrap_or_else(|err| {
            panic!(
                "generated key binding {:?} references command {} that keymap cannot invoke: {}",
                binding.matcher, binding.command_id, err
            )
        });
    }
}

fn key_binding_command_context<'a>(
    scope: KeyBindingScope,
    extensions: &'a ExtensionUiSnapshot,
) -> CommandPolicyContext<'a> {
    CommandPolicyContext {
        source: CommandInvocationSource::Keymap,
        runtime: RuntimeConditionContext {
            mode: match scope {
                KeyBindingScope::Normal => Mode::Normal,
                KeyBindingScope::Palette => Mode::Palette,
                KeyBindingScope::Help => Mode::Help,
            },
            active_palette: matches!(scope, KeyBindingScope::Palette)
                .then_some(PaletteKind::Command),
            focused_text_input: matches!(scope, KeyBindingScope::Palette),
            text_history_available: matches!(scope, KeyBindingScope::Palette),
            extensions,
        },
    }
}

#[test]
fn command_registry_metadata_has_reviewable_public_surface() {
    let specs = command_registry();
    assert!(
        !specs.is_empty(),
        "command registry must define at least one command"
    );

    for spec in specs {
        assert!(!spec.id.is_empty(), "command id must not be empty");
        assert!(
            !spec.title.is_empty(),
            "{} title must not be empty",
            spec.id
        );

        for arg in spec.args {
            assert!(
                !arg.name.is_empty(),
                "{} has an argument with an empty name",
                spec.id
            );
        }
    }
}

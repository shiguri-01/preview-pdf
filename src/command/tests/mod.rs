use crate::app::Mode;
use crate::command::{
    CommandInvocationSource, CommandPolicyContext, command_registry, find_command_spec,
};
use crate::condition::{ConditionExpr, RuntimeConditionContext, evaluate_condition};
use crate::config::keymap::build_default_sequence_registry;
use crate::extension::ExtensionUiSnapshot;
use crate::palette::PaletteKind;

use super::spec::validate_command_id_for_policy;

fn extension_snapshot(search_active: bool) -> ExtensionUiSnapshot {
    ExtensionUiSnapshot {
        search: crate::search::SearchUiSnapshot {
            active: search_active,
            ..Default::default()
        },
        ..ExtensionUiSnapshot::default()
    }
}

#[test]
fn default_registry_bindings_reference_commands_invocable_when_enabled() {
    let registry = build_default_sequence_registry();
    let snapshot = registry.snapshot();

    assert!(
        !snapshot.exact_bindings.is_empty(),
        "default registry must define at least one exact binding"
    );
    assert!(
        !snapshot.numeric_prefix_bindings.is_empty(),
        "default registry must define at least one numeric prefix binding"
    );

    for binding in snapshot.exact_bindings {
        assert!(
            find_command_spec(binding.command_id).is_some(),
            "key binding {:?} references unknown command {}",
            binding.keys,
            binding.command_id
        );
        assert_binding_is_invocable(
            binding.command_id,
            binding.enabled_when,
            format!("key binding {:?}", binding.keys),
        );
    }

    for binding in snapshot.numeric_prefix_bindings {
        assert!(
            find_command_spec(binding.command_id).is_some(),
            "numeric key binding {:?} references unknown command {}",
            binding.suffix,
            binding.command_id
        );
        assert_binding_is_invocable(
            binding.command_id,
            binding.enabled_when,
            format!("numeric key binding {:?}", binding.suffix),
        );
    }

    for binding in snapshot.generated_bindings {
        assert!(
            find_command_spec(binding.command_id).is_some(),
            "generated key binding {:?} references unknown command {}",
            binding.matcher,
            binding.command_id
        );
        assert_binding_is_invocable(
            binding.command_id,
            binding.enabled_when,
            format!("generated key binding {:?}", binding.matcher),
        );
    }
}

fn assert_binding_is_invocable(command_id: &str, enabled_when: ConditionExpr, label: String) {
    let extension_states = [extension_snapshot(false), extension_snapshot(true)];
    let contexts = extension_states.iter().flat_map(|extensions| {
        [
            RuntimeConditionContext::new(Mode::Normal, None, extensions),
            RuntimeConditionContext::new(Mode::Palette, Some(PaletteKind::Command), extensions),
            RuntimeConditionContext::new(Mode::Palette, Some(PaletteKind::Outline), extensions),
            RuntimeConditionContext::new(Mode::Help, None, extensions),
        ]
    });
    let mut enabled_context_found = false;

    for runtime in contexts {
        if !evaluate_condition(enabled_when, &runtime) {
            continue;
        }
        enabled_context_found = true;

        let ctx = CommandPolicyContext {
            source: CommandInvocationSource::Binding,
            runtime,
        };
        if validate_command_id_for_policy(command_id, &ctx).is_ok() {
            return;
        }
    }

    assert!(
        enabled_context_found,
        "{label} has an unsatisfiable enabled_when condition"
    );
    panic!("{label} references command {command_id} that bindings cannot invoke when enabled");
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

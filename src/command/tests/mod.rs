use crate::app::Mode;
use crate::command::{
    CommandInvocationSource, CommandPolicyContext, command_registry, find_command_spec,
};
use crate::condition::{ConditionExpr, RuntimeConditionContext, evaluate_condition};
use crate::extension::ExtensionUiSnapshot;
use crate::input::keymap::build_default_sequence_registry;
use crate::palette::PaletteKind;

use super::spec::validate_command_id_for_policy;

#[test]
fn default_registry_bindings_reference_commands_invocable_when_enabled() {
    let registry = build_default_sequence_registry();
    let snapshot = registry.snapshot();
    let extensions = ExtensionUiSnapshot::with_search_active(true);

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
            &extensions,
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
            &extensions,
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
            &extensions,
            format!("generated key binding {:?}", binding.matcher),
        );
    }
}

fn assert_binding_is_invocable(
    command_id: &str,
    enabled_when: ConditionExpr,
    extensions: &ExtensionUiSnapshot,
    label: String,
) {
    let contexts = [
        RuntimeConditionContext::new(Mode::Normal, None, extensions),
        RuntimeConditionContext::new(Mode::Palette, Some(PaletteKind::Command), extensions),
        RuntimeConditionContext::new(Mode::Palette, Some(PaletteKind::Outline), extensions),
        RuntimeConditionContext::new(Mode::Help, None, extensions),
    ];
    let runtime = contexts
        .into_iter()
        .find(|ctx| evaluate_condition(enabled_when, ctx))
        .unwrap_or_else(|| panic!("{label} has an unsatisfiable enabled_when condition"));
    let ctx = CommandPolicyContext {
        source: CommandInvocationSource::Binding,
        runtime,
    };
    validate_command_id_for_policy(command_id, &ctx).unwrap_or_else(|err| {
        panic!("{label} references command {command_id} that bindings cannot invoke: {err}")
    });
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

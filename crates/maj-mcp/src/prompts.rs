//! Prompts: the procedures an editor would be told on their first day.
//!
//! A tool catalogue tells a model *what it can do*. A prompt tells it *how this house does
//! it*, in the order that avoids the mistakes. Both are part of the protocol, and the
//! difference matters: a schema can say that `maj_add_effect` takes an effect and a clip,
//! but only a prompt can say that a grade should be applied clip by clip and checked against
//! a rendered frame before the next one is touched.
//!
//! The prompts here are deliberately few and specific. Every one of them names real tools,
//! works against the certified path, and assumes the engine's own rules — one command per
//! edit, no identifier guessing, a rendered frame as the only proof. A prompt that describes
//! a workflow the tools cannot perform is worse than no prompt at all.

use serde_json::{Value, json};

/// One prompt this server offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prompt {
    /// The name a client asks for.
    pub name: &'static str,
    /// A short title.
    pub title: &'static str,
    /// What the procedure is for.
    pub description: &'static str,
    /// Its arguments.
    pub arguments: &'static [Argument],
}

/// One argument of a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argument {
    /// The name.
    pub name: &'static str,
    /// What to put in it.
    pub description: &'static str,
    /// Whether the procedure needs it.
    pub required: bool,
}

/// Every prompt, in a fixed order.
pub static PROMPTS: &[Prompt] = &[
    Prompt {
        name: "maj_rough_cut",
        title: "Assemble a rough cut",
        description: "Set up a timeline and place media on it, checking each step against the \
                      project's own state rather than against an assumption.",
        arguments: &[Argument {
            name: "brief",
            description: "What the cut is for: duration, shape, and the order of the material.",
            required: false,
        }],
    },
    Prompt {
        name: "maj_color_pass",
        title: "Do a colour pass",
        description: "Grade a sequence clip by clip with the certified effects, verifying what \
                      the frame looks like after each change.",
        arguments: &[Argument {
            name: "look",
            description: "The look to aim for, in plain language.",
            required: false,
        }],
    },
    Prompt {
        name: "maj_conform_check",
        title: "Check a project conforms",
        description: "Audit a project: does its state come from its log, does every clip point \
                      at media that exists, and is anything disabled or muted by accident.",
        arguments: &[],
    },
];

/// Looks a prompt up by name.
#[must_use]
pub fn prompt(name: &str) -> Option<&'static Prompt> {
    PROMPTS.iter().find(|prompt| prompt.name == name)
}

/// Every prompt as `prompts/list` sends it.
#[must_use]
pub fn list() -> Value {
    json!({
        "prompts": PROMPTS
            .iter()
            .map(|prompt| json!({
                "name": prompt.name,
                "title": prompt.title,
                "description": prompt.description,
                "arguments": prompt.arguments
                    .iter()
                    .map(|argument| json!({
                        "name": argument.name,
                        "description": argument.description,
                        "required": argument.required,
                    }))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// One prompt rendered, as `prompts/get` answers it.
///
/// Returns `None` for a prompt this server does not offer, so a caller can turn that into the
/// protocol's own error rather than inventing an empty procedure.
#[must_use]
pub fn get(name: &str, arguments: Option<&serde_json::Map<String, Value>>) -> Option<Value> {
    let prompt = prompt(name)?;
    let text = match name {
        "maj_rough_cut" => rough_cut(brief(arguments)),
        "maj_color_pass" => color_pass(brief(arguments)),
        "maj_conform_check" => conform_check(),
        _ => return None,
    };
    Some(json!({
        "description": prompt.description,
        "messages": [{
            "role": "user",
            "content": {"type": "text", "text": text},
        }],
    }))
}

/// An argument's value, if it was given.
fn brief(arguments: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    arguments
        .and_then(|arguments| arguments.get("brief").or_else(|| arguments.get("look")))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The assembly procedure.
fn rough_cut(brief: Option<String>) -> String {
    let brief =
        brief.unwrap_or_else(|| "no brief was given; ask for one before cutting".to_owned());
    format!(
        "You are cutting on Majestat, a deterministic editing engine. Every edit you make is \
         one immutable command in a log, so anything you do can be undone exactly, reviewed \
         and replayed. Work in this order:\n\
         \n\
         1. Read `maj_project_state` and `maj_timeline`. Never name an identifier you have not \
         seen: the timeline is the only source of truth about what exists.\n\
         2. Register each source with `maj_create_media_asset` if it is not already in the \
         pool. Media this build cannot decode still renders, as a labelled chart, so register \
         it rather than working around it.\n\
         3. Set the timeline up with `maj_setup_sequence` — one call for the frame size, the \
         rate and the tracks.\n\
         4. Place material with `maj_place_clip`. It registers nothing and guesses nothing: \
         the identifier of the asset and the half-open source range `[start, end)` are yours \
         to state.\n\
         5. Look at the result with `maj_render_frame` before moving on. A cut you have not \
         seen is a cut you do not know; the returned digest is the same number `maj verify` \
         checks against a tiled render.\n\
         6. Refine with `maj_trim_clip`, `maj_move_clip`, `maj_add_transition` and \
         `maj_set_clip_speed`. Prefer one command per intent: a move and a trim are two edits, \
         and undoing one of them should leave the other standing.\n\
         \n\
         Brief: {brief}\n\
         \n\
         Finish by reporting the timeline as `maj_timeline` shows it, the render digest of \
         the frame you consider representative, and the revision number your work reached. If \
         a step was refused, report the refusal rather than working around it."
    )
}

/// The grading procedure.
fn color_pass(look: Option<String>) -> String {
    let look = look.unwrap_or_else(|| "a neutral grade; ask what look is wanted".to_owned());
    format!(
        "You are grading on Majestat's certified path. The effects available are the ten \
         built-ins, each with a declared parameter schema, and nothing else — a look that \
         needs an effect outside that list is a look this build cannot make, and saying so is \
         the correct answer.\n\
         \n\
         1. Read `maj_clip_detail` for the clip you are about to touch. It lists every \
         parameter with its minimum and maximum, so you never have to discover a legal range \
         by trial.\n\
         2. Apply the look with `maj_add_effect`, then `maj_set_effect_param` for each \
         parameter. Setting a key the effect does not declare is refused by name.\n\
         3. Grade clip by clip, not whole-sequence: the engine has no group node yet, and a \
         per-clip chain is what the certified path verifies.\n\
         4. Render every clip you change with `maj_render_frame` and compare the digest with \
         the previous one. Two clips with identical digests after a change means the change \
         did nothing.\n\
         5. When the look is right, run `maj_verify_frame` on a representative frame: it \
         renders whole and tiled and compares the digests, which is the engine's own proof \
         that the result is reproducible.\n\
         \n\
         Look: {look}\n\
         \n\
         Report the parameter values you settled on, per clip, so the grade can be reviewed \
         as a decision rather than as a pile of edits."
    )
}

/// The audit procedure.
fn conform_check() -> String {
    "Audit this project and report what you find; change nothing unless you are asked to.\n\
     \n\
     1. `maj_project_state` for the revision, the counts and the three hashes, and \
     `maj_replay_check` to confirm the state is a function of the log. A mismatch is the \
     only finding that matters: it means something changed the project without logging it.\n\
     2. `maj_timeline` for every sequence. For each clip, check that the source range lies \
     inside the asset's duration, that clips on a track do not overlap, and that a \
     transition's partners still exist and still cover it.\n\
     3. `maj_history` to read the log itself: who made each change, and whether the last few \
     form a coherent edit or an abandoned experiment.\n\
     4. Report every disabled clip, muted or locked track, and clip whose asset is missing \
     from the pool, with the identifiers needed to fix each one.\n\
     \n\
     Answer as a list of findings, each with the identifier it concerns, what is wrong, and \
     the tool call that would fix it. If the project is clean, say so and give the revision \
     and state hash you checked."
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prompt_renders_and_names_tools_that_exist() {
        for prompt in PROMPTS {
            let rendered = get(prompt.name, None).expect("the prompt renders");
            let text = rendered["messages"][0]["content"]["text"]
                .as_str()
                .expect("the message carries text");
            assert!(
                text.len() > 200,
                "{} is too thin to be a procedure",
                prompt.name
            );
            // Any tool a prompt mentions must be one the catalogue actually offers, or the
            // prompt is teaching a model to call something that does not exist.
            for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                if word.starts_with("maj_")
                    && word.len() > 4
                    && crate::catalog::tool(word).is_none()
                {
                    panic!("{} mentions `{word}`, which is not a tool", prompt.name);
                }
            }
        }
    }

    #[test]
    fn a_prompt_that_is_not_offered_has_no_rendering() {
        assert!(prompt("maj_do_the_impossible").is_none());
        assert!(get("maj_do_the_impossible", None).is_none());
    }

    #[test]
    fn an_argument_changes_the_rendered_procedure() {
        let mut arguments = serde_json::Map::new();
        arguments.insert("brief".to_owned(), Value::from("ninety seconds, cold open"));
        let rendered = get("maj_rough_cut", Some(&arguments)).expect("renders");
        let text = rendered["messages"][0]["content"]["text"]
            .as_str()
            .expect("text");
        assert!(text.contains("ninety seconds, cold open"));
    }

    #[test]
    fn the_listing_names_every_prompt_and_its_arguments() {
        let listed = list();
        let entries = listed["prompts"].as_array().expect("a list");
        assert_eq!(entries.len(), PROMPTS.len());
        assert_eq!(entries[0]["name"], json!("maj_rough_cut"));
        assert_eq!(entries[0]["arguments"][0]["required"], json!(false));
        assert_eq!(entries[2]["arguments"], json!([]));
    }
}

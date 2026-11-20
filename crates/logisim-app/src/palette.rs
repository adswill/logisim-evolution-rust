//! Command palette model: what can be searched and how results are ranked. UI-free.

use crate::actions::Action;
use crate::catalog::CatalogPart;
use crate::fuzzy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Action(Action),
    /// Start placing a part.
    Part {
        library: String,
        part: String,
    },
    OpenCircuit(String),
    PlaceCircuit(String),
    /// Open (toggle) a plug-in window by id.
    Window(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub target: Target,
    pub label: String,
    /// Secondary text: shortcut, library, description.
    pub detail: String,
}

pub fn entries(
    parts: &[CatalogPart],
    circuits: &[String],
    windows: &[(String, String, String)],
) -> Vec<Entry> {
    let mut v: Vec<Entry> = Action::ALL
        .iter()
        .filter(|a| **a != Action::CommandPalette)
        .map(|a| Entry {
            target: Target::Action(*a),
            label: a.label().to_string(),
            detail: a.shortcut().unwrap_or_else(|| a.category().to_string()),
        })
        .collect();
    v.extend(parts.iter().map(|p| Entry {
        target: Target::Part {
            library: p.library.clone(),
            part: p.name.clone(),
        },
        label: format!("Place {}", p.name),
        detail: format!("{} · {}", p.library, p.description),
    }));
    for c in circuits {
        v.push(Entry {
            target: Target::OpenCircuit(c.clone()),
            label: format!("Open circuit {c}"),
            detail: "Circuits".into(),
        });
        v.push(Entry {
            target: Target::PlaceCircuit(c.clone()),
            label: format!("Place circuit {c}"),
            detail: "Circuits".into(),
        });
    }
    for (id, title, desc) in windows {
        v.push(Entry {
            target: Target::Window(id.clone()),
            label: format!("Window: {title}"),
            detail: desc.clone(),
        });
    }
    v
}

/// Indexes of matching entries, best first. With an empty query: recently used parts first.
pub fn rank(all: &[Entry], query: &str, recent_part_ids: &[String], limit: usize) -> Vec<usize> {
    let recent_bonus = |e: &Entry| -> i32 {
        match &e.target {
            Target::Part { library, part } => {
                let id = format!("{library}/{part}");
                recent_part_ids
                    .iter()
                    .position(|r| *r == id)
                    .map_or(0, |i| 40 - 4 * i as i32)
            }
            _ => 0,
        }
    };
    let mut scored: Vec<(i32, usize)> = all
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let s = fuzzy::score(query, &e.label)
                .or_else(|| fuzzy::score(query, &e.detail).map(|s| s / 4 - 20))?;
            Some((s + recent_bonus(e), i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::all_parts;

    fn setup() -> Vec<Entry> {
        entries(
            &all_parts(),
            &["main".into(), "adder".into()],
            &[("analysis".into(), "Analysis".into(), "Truth tables".into())],
        )
    }

    #[test]
    fn typing_finds_actions_parts_and_circuits() {
        let all = setup();
        let top = |q: &str| all[rank(&all, q, &[], 5)[0]].label.clone();
        assert_eq!(top("save as"), "Save as…");
        assert_eq!(top("xnor"), "Place XNOR Gate");
        assert_eq!(top("fit"), "Fit circuit to window");
        assert!(top("open circuit add").contains("adder"));
        assert_eq!(top("theme high"), "Theme: high contrast");
        assert_eq!(top("window analysis"), "Window: Analysis");
    }

    #[test]
    fn nothing_matches_nonsense() {
        assert!(rank(&setup(), "qqzzxx", &[], 5).is_empty());
    }

    #[test]
    fn recent_parts_lead_the_empty_query() {
        let all = setup();
        let recents = vec!["Wiring/Tunnel".to_string(), "Gates/NOR Gate".to_string()];
        let r = rank(&all, "", &recents, 3);
        assert_eq!(all[r[0]].label, "Place Tunnel");
        assert_eq!(all[r[1]].label, "Place NOR Gate");
    }

    #[test]
    fn limit_is_respected_and_palette_does_not_list_itself() {
        let all = setup();
        assert_eq!(rank(&all, "", &[], 7).len(), 7);
        assert!(!all
            .iter()
            .any(|e| e.target == Target::Action(Action::CommandPalette)));
    }
}

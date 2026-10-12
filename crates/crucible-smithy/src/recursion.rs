//! Recursive components are a property of the model, independent of language boxing.
use crate::ShapeId;
use crate::ast::Shape;
use crate::ir;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecursiveGroup {
    pub shapes: Vec<ShapeId>,
    /// Members whose edges stay inside this recursive component.
    pub members: Vec<ShapeId>,
}

/// The recursive components among the Smithy shapes `reached`.
pub(crate) fn lower(model: &BTreeMap<ShapeId, Shape>, reached: &[ShapeId]) -> Vec<RecursiveGroup> {
    components(reached, |id| {
        let shape = &model[id];
        shape
            .members
            .iter()
            .map(|(name, member)| (name.as_str(), member))
            .chain(shape.member.iter().map(|member| ("member", member)))
            .chain(shape.key.iter().map(|member| ("key", member)))
            .chain(shape.value.iter().map(|member| ("value", member)))
            .map(|(name, member)| (id.with_member(name), member.target.clone()))
            .collect()
    })
}

/// The recursive components among lowered shapes: what a front end that
/// builds the IR directly (`.cru`) fills `Service::recursive_groups` with,
/// by the same algorithm the Smithy front end uses.
pub fn groups(shapes: &BTreeMap<ShapeId, ir::Shape>) -> Vec<RecursiveGroup> {
    let reached: Vec<ShapeId> = shapes.keys().cloned().collect();
    components(&reached, |id| {
        let members: Vec<&ir::Member> = match &shapes[id] {
            ir::Shape::Structure(s) | ir::Shape::Union(s) => s.members.iter().collect(),
            ir::Shape::List(list) => vec![&list.member],
            ir::Shape::Map(map) => vec![&map.key, &map.value],
            _ => Vec::new(),
        };
        members
            .into_iter()
            .map(|member| (id.with_member(&member.name), member.target.clone()))
            .collect()
    })
}

/// Kosaraju's strongly connected components of the graph whose nodes are
/// `reached` and whose edges are `edges(node)` (member id, target), keeping
/// only those that recur: more than one shape, or a shape that targets itself.
/// Sorted by shapes, so the result does not depend on the order of `reached`.
fn components(
    reached: &[ShapeId],
    edges_of: impl Fn(&ShapeId) -> Vec<(ShapeId, ShapeId)>,
) -> Vec<RecursiveGroup> {
    let nodes: BTreeSet<_> = reached.iter().cloned().collect();
    let mut edges = BTreeMap::<ShapeId, Vec<(ShapeId, ShapeId)>>::new();
    let mut reverse = BTreeMap::<ShapeId, Vec<ShapeId>>::new();
    for id in reached {
        for (member, target) in edges_of(id) {
            if nodes.contains(&target) {
                reverse.entry(target.clone()).or_default().push(id.clone());
                edges.entry(id.clone()).or_default().push((member, target));
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for id in reached {
        let mut stack = vec![(id.clone(), false)];
        while let Some((node, done)) = stack.pop() {
            if done {
                order.push(node);
                continue;
            }
            if !seen.insert(node.clone()) {
                continue;
            }
            stack.push((node.clone(), true));
            for (_, target) in edges.get(&node).into_iter().flatten().rev() {
                stack.push((target.clone(), false));
            }
        }
    }
    seen.clear();
    let mut groups = Vec::new();
    for id in order.into_iter().rev() {
        if seen.contains(&id) {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut stack = vec![id];
        while let Some(node) = stack.pop() {
            if !seen.insert(node.clone()) {
                continue;
            }
            component.insert(node.clone());
            stack.extend(reverse.get(&node).into_iter().flatten().cloned());
        }
        let members: Vec<_> = component
            .iter()
            .flat_map(|id| edges.get(id).into_iter().flatten())
            .filter(|(_, target)| component.contains(target))
            .map(|(member, _)| member.clone())
            .collect();
        if component.len() == 1 && members.is_empty() {
            continue;
        }
        groups.push(RecursiveGroup {
            shapes: component.into_iter().collect(),
            members,
        });
    }
    groups.sort_by(|a, b| a.shapes.cmp(&b.shapes));
    groups
}

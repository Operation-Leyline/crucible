//! Recursive components are a property of the model, independent of language boxing.
use crate::ast::{Kind, Shape};
use crate::{Diagnostics, ShapeId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecursiveGroup {
    pub shapes: Vec<ShapeId>,
    /// Members whose edges stay inside this recursive component.
    pub members: Vec<ShapeId>,
}

pub(crate) fn lower(
    model: &BTreeMap<ShapeId, Shape>,
    reached: &[ShapeId],
    diagnostics: &mut Diagnostics,
) -> Vec<RecursiveGroup> {
    let nodes: BTreeSet<_> = reached.iter().cloned().collect();
    let mut edges = BTreeMap::<ShapeId, Vec<(ShapeId, ShapeId)>>::new();
    let mut reverse = BTreeMap::<ShapeId, Vec<ShapeId>>::new();
    for id in reached {
        let shape = &model[id];
        let members = shape
            .members
            .iter()
            .map(|(name, member)| (name.as_str(), member))
            .chain(shape.member.iter().map(|member| ("member", member)))
            .chain(shape.key.iter().map(|member| ("key", member)))
            .chain(shape.value.iter().map(|member| ("value", member)));
        for (name, member) in members {
            if nodes.contains(&member.target) {
                edges
                    .entry(id.clone())
                    .or_default()
                    .push((id.with_member(name), member.target.clone()));
                reverse
                    .entry(member.target.clone())
                    .or_default()
                    .push(id.clone());
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
        if component
            .iter()
            .all(|id| matches!(model[id].kind, Kind::List | Kind::Set | Kind::Map))
        {
            for id in &component {
                diagnostics.push(id,"recursive collections must pass through a structure or union; a collection-only cycle is invalid Smithy");
            }
        }
        groups.push(RecursiveGroup {
            shapes: component.into_iter().collect(),
            members,
        });
    }
    groups.sort_by(|a, b| a.shapes.cmp(&b.shapes));
    groups
}

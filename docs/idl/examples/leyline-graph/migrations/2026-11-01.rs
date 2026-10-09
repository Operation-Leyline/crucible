//! What the service writes for migrations/2026-11-01.cru. Everything else in
//! that file converts automatically. The generated registry refuses to build
//! until each hook below exists, the way it refuses a missing operation today.
//!
//! Handlers never change: CreateGraph, GetGraph and ListGraphs only ever see
//! 2026-11-01 types. Older types live in generated `v2026_09_04` modules.

use crate::leyline::migrations::m2026_11_01::{
    CreateGraphUpgrade, GetWriteLegacy, GraphDataDowngrade, ListGraphsUpgrade,
};
use crate::leyline::v2026_09_04;
use crate::leyline::{ByteCount, GraphGroupId, GraphHomeRegion, GraphStorage};
use crucible_server_runtime_rust::{Result, WithContext};

use crate::operations::Leyline;

/// `require CreateGraph.input.homeRegion by async hook`: may do I/O, and the
/// generated trait says so, so it is obvious which old requests cost a read.
impl CreateGraphUpgrade for Leyline {
    async fn home_region(
        &self,
        input: &v2026_09_04::CreateGraphInput,
    ) -> Result<GraphHomeRegion> {
        let org = self.organizations.get(&input.organization_id).await?;
        Ok(org.home_region)
    }
}

/// `reshape GraphData { sizeBytes, indexBytes } -> storage by hook`: pure.
/// GraphData is only returned, so this is the only direction generated.
impl GraphDataDowngrade for Leyline {
    fn storage(storage: Option<&GraphStorage>) -> (Option<ByteCount>, Option<ByteCount>) {
        match storage {
            Some(s) => (Some(s.data_bytes), Some(s.index_bytes)),
            None => (None, None),
        }
    }
}

/// `reshape ListGraphs.input { groupId } -> groupIds by hook`: pure upgrade.
impl ListGraphsUpgrade for Leyline {
    fn group_ids(group_id: Option<GraphGroupId>) -> Option<Vec<GraphGroupId>> {
        group_id.map(|group| vec![group])
    }
}

/// `remove operation GetWrite`: the one case with a second handler. It speaks
/// only the old types and is only reachable at versions before 2026-11-01.
impl GetWriteLegacy for Leyline {
    async fn get_write(
        &self,
        input: v2026_09_04::GetWriteInput,
    ) -> Result<v2026_09_04::GetWriteOutput> {
        let write = self.writes.get(&input.graph_id, &input.write_id).await?;
        Ok(v2026_09_04::GetWriteOutput { status: write.status.into() })
    }
}

// `behavior ListGraphs.excludesRetiring`, inside the ordinary ListGraphs
// handler (operations/graph/list_graphs.rs):
//
//     let include_retiring =
//         self.context().api_version() < ApiVersion::V2026_11_01;
//     let graphs = self.graphs.list(&input, include_retiring).await?;
//
// A rule can require every version comparison in handlers to name a version
// that declares a `behavior`, so branches cannot appear without a changelog
// entry.

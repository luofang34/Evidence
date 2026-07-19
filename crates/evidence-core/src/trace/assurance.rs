//! Canonical assurance mappings projected from the corpus graph.

use std::collections::{BTreeMap, BTreeSet};

use crate::corpus::{CorpusGraph, Node, RequirementLayer, TraceMetadata};

use super::{HlrEntry, KNOWN_SURFACES, LinkError};

/// Missing and unknown values in a two-way assurance mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BijectionDiff {
    /// Values declared by graph requirements but absent from the
    /// authoritative catalog.
    pub unknown: Vec<String>,
    /// Catalog values with no graph requirement claimant.
    pub unclaimed: Vec<String>,
}

/// Canonical surface and diagnostic-code claimants keyed by claimed
/// value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssuranceBijections {
    surface_claimants: BTreeMap<String, BTreeSet<String>>,
    diagnostic_claimants: BTreeMap<String, BTreeSet<String>>,
}

impl AssuranceBijections {
    /// Project assurance mappings from HLR and LLR graph nodes.
    pub fn from_graph(graph: &CorpusGraph) -> Self {
        let mut mappings = Self::default();
        for node in graph.nodes() {
            let Node::Requirement(requirement) = node else {
                continue;
            };
            let Some(TraceMetadata::Requirement(metadata)) = graph.trace_metadata(&requirement.uid)
            else {
                continue;
            };
            match requirement.layer {
                RequirementLayer::Hlr => add_claims(
                    &mut mappings.surface_claimants,
                    &requirement.id,
                    &metadata.surfaces,
                ),
                RequirementLayer::Llr => add_claims(
                    &mut mappings.diagnostic_claimants,
                    &requirement.id,
                    &metadata.emits,
                ),
                RequirementLayer::Source | RequirementLayer::Sys | RequirementLayer::Derived => {}
            }
        }
        mappings
    }

    pub(crate) fn from_hlr_entries(entries: &[HlrEntry]) -> Self {
        let mut mappings = Self::default();
        for entry in entries {
            add_claims(&mut mappings.surface_claimants, &entry.id, &entry.surfaces);
        }
        mappings
    }

    /// Surface name to the set of HLR human identifiers that claim it.
    pub fn surface_claimants(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.surface_claimants
    }

    /// Diagnostic code to the set of LLR human identifiers that claim it.
    pub fn diagnostic_claimants(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.diagnostic_claimants
    }

    /// Compare graph-derived diagnostic claims with the RULES catalog.
    pub fn diagnostic_diff(&self, known: &[&str], reserved: &[&str]) -> BijectionDiff {
        let known: BTreeSet<&str> = known.iter().copied().collect();
        let reserved: BTreeSet<&str> = reserved.iter().copied().collect();
        let unknown = self
            .diagnostic_claimants
            .keys()
            .filter(|code| !known.contains(code.as_str()))
            .cloned()
            .collect();
        let unclaimed = known
            .into_iter()
            .filter(|code| !reserved.contains(code))
            .filter(|code| !self.diagnostic_claimants.contains_key(*code))
            .map(str::to_string)
            .collect();
        BijectionDiff { unknown, unclaimed }
    }

    pub(crate) fn surface_errors(&self) -> Vec<LinkError> {
        let known: BTreeSet<&str> = KNOWN_SURFACES.iter().copied().collect();
        let mut errors = Vec::new();
        for (surface, claimants) in &self.surface_claimants {
            if !known.contains(surface.as_str()) {
                for hlr_id in claimants {
                    errors.push(LinkError::SurfaceUnknown {
                        hlr_id: hlr_id.clone(),
                        surface: surface.clone(),
                    });
                }
            }
        }
        for surface in KNOWN_SURFACES {
            if !self.surface_claimants.contains_key(*surface) {
                errors.push(LinkError::SurfaceUnclaimed {
                    surface: (*surface).to_string(),
                });
            }
        }
        errors
    }
}

fn add_claims(
    mappings: &mut BTreeMap<String, BTreeSet<String>>,
    requirement_id: &str,
    claims: &[String],
) {
    for claim in claims {
        mappings
            .entry(claim.clone())
            .or_default()
            .insert(requirement_id.to_string());
    }
}

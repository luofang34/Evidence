//! rmcp version/protocol guardrail on the `initialize` handshake.
//!
//! The MCP `protocolVersion` the server negotiates comes from rmcp's
//! `ServerHandler` default, so a bump of the `=`-pinned `rmcp`
//! dependency can shift it without any evidence-mcp code change. These
//! tests pin the negotiation — a supported request is echoed, an
//! unsupported one falls back to rmcp's latest — so such a bump fails
//! here for review instead of shipping a silent handshake change.
//! Shares the `helpers` module via `#[path]`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

#[path = "mcp_surface/helpers.rs"]
mod helpers;

use helpers::{init_frames_with_protocol, session};

/// TEST-118 selector: a supported protocol (`2024-11-05`, the version
/// our agents speak) is echoed; an unsupported request falls back to
/// the rmcp build's latest supported protocol, pinned here.
#[test]
fn initialize_protocol_negotiation_is_pinned() {
    assert_eq!(
        negotiated_protocol("2024-11-05"),
        "2024-11-05",
        "server must still echo the 2024-11-05 protocol our agents use"
    );
    assert_eq!(
        negotiated_protocol("9999-99-99"),
        "2025-11-25",
        "rmcp's latest supported protocol moved off 2025-11-25 — review the rmcp bump"
    );
}

/// The `protocolVersion` an `initialize` handshake negotiates when the
/// client requests `requested`.
fn negotiated_protocol(requested: &str) -> String {
    let frames = init_frames_with_protocol(requested);
    let responses = session(&frames, 1);
    responses[0]
        .pointer("/result/protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("missing result.protocolVersion: {:?}", responses[0]))
        .to_string()
}

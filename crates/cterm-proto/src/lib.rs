//! cterm-proto: Protobuf definitions and type conversions for the cterm gRPC protocol
//!
//! This crate contains the shared protocol definitions used by both ctermd (daemon)
//! and cterm (UI client) for communication over Unix sockets or SSH.

pub mod convert;

/// Generated protobuf and gRPC code
pub mod proto {
    tonic::include_proto!("cterm.terminal");
}

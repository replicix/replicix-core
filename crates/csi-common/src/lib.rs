// SPDX-License-Identifier: MPL-2.0
//! What two Kubernetes CSI drivers share: the CSI v1 protobuf/gRPC bindings
//! ([`proto`]), `StorageClass` parameter parsing helpers ([`params`]), the
//! volume-id codecs ([`volume_id`]) and the credentials Secret watch
//! ([`credentials`]).

pub mod credentials;
pub mod params;
pub mod proto;
pub mod volume_id;

//! wash-fps `client/ragdoll`: what a dying body does before it goes limp.
//!
//! `RagdollFrame.ts` is not ported: it gives Babylon's ragdoll bone setters a
//! frame whose scale matches the rig's export scale, which shagger's ragdoll
//! (posed from the body matrix directly) does not need.

pub mod death_clip;

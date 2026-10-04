#![allow(dead_code)]
struct OtherCapability;
struct FakeActualCapability;
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ActualCapability",
    family = "runtime_helper"
)]
fn string_marker() {
    let _CAPABILITY = "ActualCapability";
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ActualCapability",
    family = "runtime_helper"
)]
fn unrelated_type(_: &OtherCapability) {}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ActualCapability",
    family = "runtime_helper"
)]
fn substring_type(_: &FakeActualCapability) {}
fn main() {}

struct ActualCapability;
mod unrelated {
    pub struct Result<T>(std::marker::PhantomData<T>);
    pub struct Arc<T>(std::marker::PhantomData<T>);
    pub struct AgentResult<T>(std::marker::PhantomData<T>);
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ActualCapability",
    family = "runtime_helper"
)]
fn qualified_result(_: unrelated::Result<ActualCapability>) {}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ActualCapability",
    family = "runtime_helper"
)]
fn qualified_arc(_: unrelated::Arc<ActualCapability>) {}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ActualCapability",
    family = "runtime_helper"
)]
fn qualified_agent_result(_: unrelated::AgentResult<ActualCapability>) {}

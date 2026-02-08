mod bwrap;
mod container;
mod native;

pub use bwrap::BwrapRuntime;
pub use container::ContainerRuntime;
pub use native::NativeRuntime;

#[derive(Debug, Clone)]
pub enum Runtime {
    Native,
    Podman { image_tag: String },
    Docker { image_tag: String },
    Bwrap { image_tag: String },
}

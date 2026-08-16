//! Audiophile inspection subsystem covering ALSA, PipeWire graph, and bit-perfect validation.

pub mod alsa;
pub mod pipewire;
pub mod status;

pub use alsa::{read_dac_capabilities, read_hw_params, scan_alsa_cards, AlsaCard, AlsaHwParams, DacCapabilities};
pub use pipewire::{inspect_pipewire_graph, CulpritClient, PipeWireGraphReport, PipeWireSink, PipeWireStream};
pub use status::{BitPerfectVerdict, PipelineStatus};

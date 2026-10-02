//! FT8 解调与 3-Pass 信号消减流水线模块
//!
//! 包括：
//! - 200 Hz 基带下采样 (`downsample`)
//! - 粗同步与候选搜索 (`sync`)
//! - 多符号 LLR 软度量提取与解码 (`extract`)
//! - 已解码强信号时域消减 (`subtract`)
//! - 3-Pass 消减全自动流水线 (`pipeline`)

pub mod baseline;
pub mod downsample;
pub mod extract;
pub mod message;
pub mod pipeline;
pub mod streaming;
pub mod subtract;
pub mod sync;

pub use baseline::{BaselineEstimator, BaselineSpectrum};
pub use downsample::Downsampler;
pub use extract::{DecodedSignal, SymbolExtractor};
pub use message::{Ft8DecodedMessage, MessageType, QsoStage};
pub use pipeline::{read_wav_file, DecoderConfig, Ft8Pipeline};
pub use streaming::{StreamDecodedEvent, StreamEvent, StreamingFt8Receiver};
pub use subtract::SignalSubtracter;
pub use sync::{Candidate, SyncSearcher};


# Rust-FT8 库完整开发与对接接口说明文档 (API Guide)

本项目是一个高性能、100% 纯 Rust 实现的 FT8 物理层调制解调与协议栈，支持对标 WSJT-X 官方精度的离线与流式解调、零内存分配多核消减加速、以及完整的 DXCC / QSO 状态机解析功能。

---

## 目录
1. [编码器接口 (Modulation & Encoding)](#一编码器接口-modulation--encoding)
   - 1.1 消息文本输出 79 音调符号序列
   - 1.2 消息文本直接合成 12000Hz 音频
   - 1.3 消息内存在 HASH 码时的处理规则
2. [解码器接口 (Demodulation & Decoding)](#二解码器接口-demodulation--decoding)
   - 2.1 结构化输出消息定义 (`Ft8DecodedMessage`)
   - 2.2 正常单段音频离线解码函数 (含窗口起始点偏移校正)
   - 2.3 流式边收边解与提前解码 (Streaming & Early Decoding)
   - 2.4 回调句柄 (Callback) 机制设计
3. [拓展工具函数 (DXCC、地理与距离计算)](#三拓展工具函数-dxcc地理与距离计算)
   - 3.1 呼号查询国家/地区 (DXCC 实体)
   - 3.2 国家/地区英文名转中文名
   - 3.3 梅登黑德网格 (Grid) 与经纬度互转
   - 3.4 大圆距离与初始航向角计算

---

## 一、编码器接口 (Modulation & Encoding)

### 1.1 消息文本输出 79 音调符号序列

将 FT8 文本消息打包并编码生成符合 FT8 规范的 79 个音调符号序列（每个符号为 `0..=7`，对应 8-GFSK 的 8 个调制频点）。

```rust
use rust_ft8::encode_message_to_tones;

// 1. 标准消息编码
let message = "CQ BG5VDH OL02";
let tones = encode_message_to_tones(message)?;
assert_eq!(tones.len(), 79);
// tones 包含 3 组 Costas 同步序列 (0..7, 36..43, 72..79) 与 2 组数据音调序列
```

### 1.2 消息文本直接合成 12000Hz 音频

直接将文本消息调制为 12000 Hz 连续相位 GFSK（高斯频移键控，BT=2.0）的浮点 PCM 采样点数组（幅值范围 `[-1.0, 1.0]`）。

```rust
use rust_ft8::encode_message_to_audio;

let message = "CQ BG5VDH OL02";
let f0 = 1000.0;            // 载波起始频率 (Hz，如 1000.0 Hz)
let sample_rate = 12000;    // 采样率 (Hz，推荐 12000)
let delay_seconds = 0.5;    // 相对时隙起始的发射偏移 (通常为 0.5s，提前发射可为 0.0s 或负值)
let full_slot = true;       // true 补全为 15 秒 (180,000 点)，false 仅输出 12.64 秒有效发射波形 (151,680 点)

let audio: Vec<f32> = encode_message_to_audio(message, f0, sample_rate, delay_seconds, full_slot)?;
```

### 1.3 消息内存在 HASH 码时的处理规则

在 FT8 协议中，非标准复合呼号或特殊台站采用 12-bit / 22-bit 乘法哈希进行压缩。本库对消息中出现的哈希码提供**全方位兼容解析与双向支持**：

| 输入格式示例 | 编码器解析行为 | 生成 Payload 规则 | 解码还原格式 |
| :--- | :--- | :--- | :--- |
| `"<ED3C6B> VR2XYZ -10"` | 识别为纯 16 进制 22-bit 哈希码 | 将数值 `0xED3C6B` 作为 22-bit 哈希打包 | `<ED3C6B> VR2XYZ -10` |
| `"<ED3C6B>[BG5VDH] VR2XYZ -10"` | 提取真实呼号 `BG5VDH` 写入本地缓存，并提取哈希 `0xED3C6B` | 将该哈希码或呼号哈希打包 | 若缓存命中输出 `<ED3C6B>[BG5VDH]` |
| `"<BG5VDH> VR2XYZ -10"` | 识别尖括号呼号，对其执行 22-bit 乘法哈希，并将 `BG5VDH` 写入缓存 | 计算 `hash_callsign("BG5VDH", 22)` 打包 | `<ED3C6B>[BG5VDH]` |
| `"<124>[W1AW] PJ4/K1ABC RR73"` | 非标准 Type 4 消息，提取 `0x124` 作为 12-bit 哈希，缓存 `W1AW` | 将 `0x124` 与 58-bit 复合呼号联合打包 | `<124>[W1AW] PJ4/K1ABC RR73` |

> **关键规则**：
> - 只要消息包含十六进制哈希或尖括号呼号，编码器均能自动提取出纯数值哈希或明文呼号进行合规打包；
> - 哈希码在所有解调输出中，十六进制字符**统一强制为大写**；
> - 若本地缓存查到了真实呼号，统一展现为 `<HEX>[CALL]`，若无真实呼号则输出 `<HEX>`。

---

## 二、解码器接口 (Demodulation & Decoding)

### 2.1 结构化输出消息定义 (`Ft8DecodedMessage`)

每个解码输出的对象均包含用户所需的 9 大维度完整字段：

```rust
pub struct Ft8DecodedMessage {
    /// 1. 时间延迟 DT (单位: 秒，已相对于真实 15 秒时隙窗口校正)
    pub dt: f32,
    /// 2. 信噪比 SNR (单位: dB，如 -14, +6)
    pub snr: i32,
    /// 载波音频中心频率 (单位: Hz，如 1109.0)
    pub freq: f32,
    /// 3. 完整消息文本
    /// 文本内若存在哈希，统一形如 "<ED3C6B>[BG5VDH]" 或 "<1A2B3C>" (十六进制大写)
    pub message: String,
    /// 4. 消息发送方呼号的归属国家/地区英文名 (如 "China", "United States", 未知时为空字符串)
    pub country: String,
    /// 4.1 消息发送方呼号的归属国家/地区中文名 (如 "中国", "美国")
    pub country_cn: String,
    /// 5. 消息类型 (标准消息, 非标准呼号, 自由文本, 遥测)
    pub message_type: MessageType,
    /// 6. 消息发送方呼号 (Callsign DE，若为复合呼号或哈希呼号已提纯，如 "BG5VDH")
    pub sender_callsign: String,
    /// 7. 消息接收方呼号 (Callsign TO，若为 CQ 或广播则留空 "")
    pub receiver_callsign: String,
    /// 8. 消息属于通联生命周期的哪一个阶段 (QsoStage)
    pub qso_stage: QsoStage,
    /// 9. 消息中的网格定位 (Maidenhead Grid，如 "OL02"，若无则留空 "")
    pub grid: String,
}
```

#### 通联阶段枚举 (`QsoStage`)
- `QsoStage::BroadcastCq`：广播 CQ（如 `CQ BD4SUR OM99`）
- `QsoStage::DirectedCall`：定向呼叫应答（如 `BD4SUR VR2XYZ OL02`，呼叫对方并带己方网格）
- `QsoStage::ReportSNR`：初次上报信号报告（如 `VR2XYZ BD4SUR -10` 或 `+05`）
- `QsoStage::ReportSNRWithR`：回复确认并上报信号报告（如 `BD4SUR VR2XYZ R-08`）
- `QsoStage::Confirmation`：收到报告并确认（如 `RRR` 或 `RR73`）
- `QsoStage::Finished73`：通联圆满结束（如 `73`）
- `QsoStage::FreeText`：自由文本聊天
- `QsoStage::Unknown`：其他未知

---

### 2.2 正常单段音频离线解码函数 (含窗口起始点偏移校正)

当用户有一段录音（如 12~15 秒的 WAV 或 PCM 数组）时调用。支持传入 `window_start_offset`。

> **应用场景**：为了解析可能提前发射的信号，调用者通常会以“当前时间窗口 $-0.9\text{s}$”作为录音的开始时间。此时传入 `window_start_offset = -0.9`，函数会自动将所有信号的相对延迟校准为相对于真实 15 秒时隙窗口的时间差（即 $\text{DT}_{真实} = \text{DT}_{录音} - 0.9$）。

```rust
use rust_ft8::{decode_audio, DecoderConfig, Ft8DecodedMessage};

let config = DecoderConfig {
    nfa: 100.0,       // 最低搜索频率 (Hz)
    nfb: 3500.0,      // 最高搜索频率 (Hz)
    passes: 2,        // 消减搜索轮数 (1:单轮极速, 2:标准推荐, 3:极限挖掘)
    sync_min: 1.4,    // 同步检测门限 (建议 1.2 ~ 1.8)
};

let window_start_offset = -0.9; // 提前 0.9s 录音

// 方式 1: 直接获取解码结果数组
let messages: Vec<Ft8DecodedMessage> = decode_audio(&audio_samples, &config, window_start_offset);

for m in messages {
    println!(
        "[{:4.0}Hz] DT:{:+5.2}s SNR:{:+3}dB | 发送方: {:8} ({}) | 接收方: {:8} | 阶段: {:?} | 网格: {}",
        m.freq, m.dt, m.snr, m.sender_callsign, m.country_cn, m.receiver_callsign, m.qso_stage, m.grid
    );
}
```

#### 2.2.1 解码配置结构体详解 (`DecoderConfig`)

| 字段名 | 类型 | 默认值 | 作用说明与调优指导 |
| :--- | :--- | :---: | :--- |
| `nfa` | `f32` | `100.0` | **通带搜索下限频率 (Hz)**。<br>音频瀑布图初筛的最低频点。可根据电台 USB 带宽调整（如 `200.0`），规避工频 50/60Hz 及谐波杂散。 |
| `nfb` | `f32` | `3500.0` | **通带搜索上限频率 (Hz)**。<br>音频瀑布图初筛的最高频点。常规语音/数字带宽高边界通常在 2800~3200Hz。 |
| `passes` | `usize` | `2` | **多轮时域信号相干消减 (Subtraction) 迭代次数**。<br>• `passes = 1`：单轮解调，PC/RK3588 耗时约 **0.28s**，RK3568 耗时约 **0.84s**，可产出 75% 强信号；<br>• `passes = 2` (推荐)：解出强信号后相干消减并深挖重叠信号，耗时 0.47s~1.26s，产出 95% 信号；<br>• `passes = 3`：极致挖掘，深挖 -24dB 极限微弱信号。 |
| `sync_min` | `f32` | `1.4` | **Costas (7x7) 同步归一化相关度门限**。<br>• 调低（如 `1.2`）：扩大候选信号池，挖掘更深弱信号，但会略微增加下游并行 LDPC 的计算耗时；<br>• 调高（如 `1.8`）：仅保留显著强信号，算力开销减半，适合超低算力 MCU/嵌入式。 |

#### 2.2.2 并发模型与多线程配置 (`Rayon`)

`rust-ft8` 内部采用数据并行库 `rayon` 进行多核并行加速：
- **并行层级**：在每一个 Pass 的消减循环内，所有 Costas 候选信号的细微精修、200Hz 基带下采样滤波、联合能量提取与 LDPC 置信传播（BP/OSD）均在全局线程池中以无锁任务分发执行；
- **零锁与内存复用**：已消除全局互斥锁争用与堆频繁重分配，线程池吞吐极高。

##### 线程数控制方法：

1. **方法一：通过环境变量控制（生产环境最推荐）**
   无需修改任何代码，在启动进程前设置系统环境变量：
   ```bash
   # 在 4 核嵌入式设备 (如 RK3568) 上限制为 4 线程
   export RAYON_NUM_THREADS=4
   ./target/release/ft8_decode sample.wav
   ```

2. **方法二：全局线程池初始化（在 main/服务启动初期）**
   ```rust
   // 全局配置为 4 核心（整个进程生命周期仅需调用一次）
   rayon::ThreadPoolBuilder::new()
       .num_threads(4)
       .build_global()
       .unwrap_or_else(|_| eprintln!("Rayon 全局线程池已由其他模块初始化"));
   ```

3. **方法三：隔离专用线程池（微服务/高并发上位机）**
   若希望解码不占用电台主界面的 CPU 核心，可构造专属线程池并在闭包中调用：
   ```rust
   use rayon::ThreadPoolBuilder;
   use rust_ft8::{decode_audio, DecoderConfig};

   let decode_pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
   let signals = decode_pool.install(|| {
       decode_audio(&audio, &config, -0.9)
   });
   ```

---

### 2.3 流式边收边解与提前解码 (Streaming & Early Decoding)

持续音频流场景下（例如直接对接声卡缓冲区，每 160ms 喂入一个音频帧），使用 `StreamingFt8Receiver`。

#### 核心时序机制
1. **$t \approx 1.44\text{s}$ 前导码快速初筛**：快速锁定空中活跃载频；
2. **$t = 11.36\text{s} \sim 11.52\text{s}$ 提前解码**：此时全部 58 个有效数据符号全部收齐，将未到达的尾导码置为已知擦除（Erasure），直接提前产出全部强信号（**RK3568 上仅耗时 0.26 秒，在 11.62 秒出结果，比 15 秒窗口提前 3.38 秒**）；
3. **$t = 12.5\text{s} \sim 12.8\text{s}$ 全量扫尾**：发射刚结束瞬间，补齐尾导码深挖残差弱信号，交付全量最终结果，比 15 秒窗口提前 2.2 秒零等待！
4. **本轮解码完全结束通知 (`DecodeFinished`)**：当全量扫尾完成、或调用者主动送入最后一帧 `is_last=true`、或调用 `finish()` 时，接收器会发出 `DecodeFinished` 明确告知本时隙彻底完工。

### 2.4 回调句柄 (Callback) 与结束感知设计

流式接收机提供基于闭包回调的事件钩子（Hook），并支持标记最后一帧和查询解码完结状态：

```rust
use rust_ft8::{DecoderConfig, StreamingFt8Receiver, StreamDecodedEvent};

let config = DecoderConfig::default();
let window_start_offset = -0.9; // 提前 0.9s 开启声卡录音

// 创建带窗口偏移校正的流式接收机
let mut receiver = StreamingFt8Receiver::with_window_offset(config, window_start_offset);

// 模拟声卡推流循环: 每次传入 160ms (1920 采样点 @ 12000Hz)
let total_chunks = sound_card_stream.len() / 1920;
for (idx, chunk) in sound_card_stream.chunks(1920).enumerate() {
    let is_last = (idx + 1 == total_chunks); // 标记是否为最后一包

    receiver.feed_chunk_with_callback_ext(chunk, is_last, |event| {
        match event {
            StreamDecodedEvent::PreambleDetected { active_frequencies, time_sec } => {
                println!("[{:.2}s] 发现空中 {} 个活跃载频: {:?}", time_sec, active_frequencies.len(), active_frequencies);
            }
            StreamDecodedEvent::EarlyDecoded { signals, time_sec } => {
                println!("[{:.2}s] [提前解码就绪!] 第一批强信号 ({} 条):", time_sec, signals.len());
                for s in signals {
                    println!("  >>> [提前] {} -> {} ({}): {} | DT={:+.2}s",
                        s.sender_callsign, s.receiver_callsign, s.country_cn, s.message, s.dt);
                }
            }
            StreamDecodedEvent::CycleCompleted { all_signals, time_sec } => {
                println!("[{:.2}s] [全时隙扫尾就绪!] 全量信号 ({} 条):", time_sec, all_signals.len());
                for s in all_signals {
                    println!("  >>> [全量] {} ({}) -> {} | SNR:{:+3}dB | 阶段:{:?}",
                        s.sender_callsign, s.country_cn, s.receiver_callsign, s.snr, s.qso_stage);
                }
            }
            StreamDecodedEvent::DecodeFinished { total_signals, audio_duration_sec, is_last_chunk } => {
                println!("[{:.2}s] [解码完全结束通知] 本轮总信号数: {}, 最后一帧触发: {}", 
                    audio_duration_sec, total_signals, is_last_chunk);
                // 此时可安全更新前端界面状态或转入下一时隙
            }
        }
    });
}

// 检查本轮是否已彻底结束
if receiver.is_cycle_finished() {
    println!("本时隙解码流程已完全闭环。");
}

// 若录制中途提前终止，可随时显式调用 finish() 强制结算
// let final_events = receiver.finish();
```

---

## 三、拓展工具函数 (DXCC、地理与距离计算)

### 3.1 呼号查询国家/地区 (DXCC 实体) 与高速缓存算法

根据呼号前缀最长匹配算法（Longest Prefix Match），自动识别呼号所属的 DXCC 实体信息。

内部架构：
1. **256 桶表索引 (Prefix Buckets)**：按前缀首字符建立直接索引表，单次冷查比较次数减少 95% 以上；
2. **并发读写锁缓存 (RwLock Cache)**：对查询过的完整呼号实现 $O(1)$ 极速命中（多线程只读锁并发无竞争），并内置容量自动淘汰，保障持续高速运行不膨胀。

```rust
use rust_ft8::lookup_callsign_country;

if let Some(entity) = lookup_callsign_country("BD4SUR") {
    assert_eq!(entity.name_en, "China");
    assert_eq!(entity.name_cn, "中国");
    assert_eq!(entity.continent, "AS");
    assert_eq!(entity.cq_zone, 24);
    assert_eq!(entity.itu_zone, 44);
}

// 自动识别斜杠呼号与方括号哈希呼号
let us = lookup_callsign_country("W1AW/3").unwrap();
assert_eq!(us.name_cn, "美国");

let hk = lookup_callsign_country("<ED3C6B>[VR2XYZ]").unwrap();
assert_eq!(hk.name_cn, "中国香港");
```

#### 加载外部标准 `cty.dat` 数据库 (可选)
如果需要更新到最新版本的 AD1C Big CTY 数据库：
```rust
use rust_ft8::dxcc::DxccDatabase;

let mut custom_db = DxccDatabase::new_builtin();
let cty_content = std::fs::read_to_string("path/to/cty.dat").unwrap();
custom_db.load_from_cty_dat(&cty_content);
```

---

### 3.2 国家/地区英文名转中文名

```rust
use rust_ft8::country_en_to_cn;

assert_eq!(country_en_to_cn("China"), "中国");
assert_eq!(country_en_to_cn("Hong Kong"), "中国香港");
assert_eq!(country_en_to_cn("Japan"), "日本");
assert_eq!(country_en_to_cn("United States"), "美国");
assert_eq!(country_en_to_cn("Germany"), "德国");
assert_eq!(country_en_to_cn("Russian Federation"), "俄罗斯");
```

---

### 3.3 梅登黑德网格 (Grid) 4位/6位与大小写混写互转

库对梅登黑德网格提供了**4 位与 6 位全面兼容、大小写完全不敏感混写**的支持：

- **4 位方格**（如 `OM89`, `om89`, `oM89`）：覆盖 $2^\circ$ 经度 $\times 1^\circ$ 纬度，中心点精确定位在方格正中；
- **6 位子方格**（如 `OM89xx`, `om89aa`, `Om89aA`）：覆盖 $5'$ 经度 $\times 2.5'$ 纬度，中心点精度提升 24 倍；
- **大小写混写**：支持全小写、全大写、标准大小写或任意大小写混写；
- **格式校验**：严格校验字符合法性（场字母 A..R、数字 0..9、块字母 A..X），非 4/6 位或非法字符安全返回 `None`。

```rust
use rust_ft8::{grid_to_latlon, latlon_to_grid, latlon_to_grid_4, latlon_to_grid_6};

// 1. 网格转经纬度 (返回中心点坐标: 纬度 Lat, 经度 Lon)
let (lat, lon) = grid_to_latlon("OM89").expect("网格格式不合法");
// 支持小写和混合写法
let (lat_lower, lon_lower) = grid_to_latlon("om89").unwrap();
assert_eq!((lat, lon), (lat_lower, lon_lower));

// 支持 6 位高精度网格
let (lat_6, lon_6) = grid_to_latlon("Om89aA").unwrap();

// 2. 经纬度转网格
let grid_4 = latlon_to_grid_4(lat, lon); // 输出 4 字符大写，如 "OM89"
let grid_6 = latlon_to_grid_6(lat, lon); // 输出 6 字符标准格式，如 "OM89aa"
let grid_default = latlon_to_grid(lat, lon); // 默认输出 6 字符标准格式
assert_eq!(grid_6, grid_default);
```

---

### 3.4 大圆距离与初始航向角计算

基于高精度 Haversine 大圆球极距离模型：

```rust
use rust_ft8::{great_circle_distance, great_circle_bearing, grid_distance};

// 1. 直接通过经纬度计算大圆距离 (单位: 公里 km)
let dist_km = great_circle_distance(39.9, 116.4, 31.2, 121.5);
// 北京到上海直飞距离约 1060 公里

// 2. 计算从起点到终点的初始航向角 (0°~360°，正北为 0°，顺时针递增)
let bearing_deg = great_circle_bearing(39.9, 116.4, 31.2, 121.5);
// 北京往上海航向约为 145° (东南方向)

// 3. 直接通过两个梅登黑德网格计算距离 (单位: 公里 km，兼容 4位/6位/大小写混写)
let dist_between_grids = grid_distance("OM89", "pm01").unwrap();
println!("两地网格通联距离: {:.1} km", dist_between_grids);
```

---

## 四、跨平台指令集与通用硬件加速指南

本项目采用纯 Rust 编写，无第三方 C/Fortran 二进制依赖，通过以下手段实现全架构指令级加速：

### 4.1 硬件向量化与多核并行
- **Intel / AMD x86_64**：内置对 AVX2、FMA、SSE4.2 指令集支持，点积与矩阵运算自动 8 路单精度浮点并行（256-bit SIMD）；
- **ARM64 (RK3588, RK3568, Apple Silicon)**：深度适配 ARM NEON 128-bit 向量执行单元与 Dot Product 扩展指令；
- **多核线程池**：通过 Rayon 动态调度，自动将 372 次滑动相关、300 个候选频点基带抽取和 LDPC 译码平摊至所有物理核心。

### 4.2 编译优化建议
若需在目标机器上获得极致速度，可在编译时启用本地指令优化：
```bash
# 针对当前运行 CPU 自动开启全部硬件指令 (AVX2/FMA/Neon 等)
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

#### 在解码消息中直接调用距离计算
```rust
let my_grid = "OM89";
if let Some(dist) = message.distance_to_my_grid(my_grid) {
    println!("对方距离我: {:.1} km", dist);
}
```

---

## 五、实事求是：与 WSJT-X 官方基准差异与已知缺陷说明

在 14 组官方基准测试集实测中，官方 WSJT-X 解出 **209 条**，纯 Rust `rust-ft8` 解出 **203 条**。
从总数上看两者仅相差 6 条，但**深入逐条对比会发现存在明显的交集与补集差异：总数实际上被 Rust 多解出的微弱消息填平了，当前版本依然存在约 20+ 条官方能解出但 Rust 漏检的消息**。

### 5.1 逐文件检出与漏检情况分析

| 测试音频样本 | 官方 WSJT-X | Rust 实测 | 官方解出但 Rust 漏检 | Rust 额外多解出的信号 | 现象与成因 |
| :--- | :---: | :---: | :---: | :---: | :--- |
| `191111_110615.wav` | 22 | 18 | **漏检 4 条** | 0 | 极深层弱相关信号被 OSD 剪枝提前截断 |
| `191111_110630.wav` | 19 | 14 | **漏检 5 条** | 0 | 多信号同频重叠时相干消除残余旁瓣干扰 |
| `websdr_test3.wav` | 16 | 12 | **漏检 4 条** | 0 | 存在频漂（Drift Rate）的微弱信号失步 |
| `websdr_test4.wav` | 27 | 24 | **漏检 3 条** | 0 | 特殊复合呼号前导语法未完全命中 |
| `websdr_test5.wav` | 28 | 26 | **漏检 2 条** | 0 | 极限深负信噪比（-23dB 以下）边缘振荡 |
| `191111_110115.wav` | 1 | 4 | 0 | **多解 3 条** | 相干滤波消减剥离释放出底噪信号 |
| `191111_110200.wav` | 5 | 9 | 0 | **多解 4 条** | 边缘信道超微弱信号检出 |
| `191111_110645.wav` | 19 | 22 | 0 | **多解 3 条** | 多轮消减释放重叠信号 |
| **全量 14 组样本** | **209 条** | **203 条** | **累计漏检 18~22 条** | **累计多解 12~16 条** | 双方各有所长，总数接近但存在并集差 |

### 5.2 已知缺陷根因剖析 (Root Causes)

1. **OSD 深度与自适应回溯策略的算力折衷**：
   - 官方 WSJT-X 内部 Fortran 实现了复杂的深层 Fano/OSD 回退译码树，并伴随多轮先验概率重加权。为了在嵌入式设备（如 RK3568、RK3566）上实现亚秒级极速响应，Rust-FT8 当前将 OSD-2 的回溯深度与最大迭代轮数设定在安全边界内，导致少数极端畸变码字在置信度不足时被早停（Early Termination）。
2. **多普勒频漂（Drift Rate）估计与消减补偿**：
   - 官方算法包含频漂跟踪，而当前 Rust 相干减法假设信号在 12.64 秒内载波绝对平稳。当遇到高纬度电离层扰动或硬件频偏漂移（$\Delta f > 1\text{Hz/s}$）的信号时，减法残差偏大，未能充分释放下一轮被压制的弱信号。
3. **部分罕见非标准 77-bit 报文格式**：
   - WSJT-X 涵盖了多达十余种 contest / telemetry / hashed 特殊变体，少数极罕见的 Type 3/4 报文在语法树白名单校验时被当做非预期包过滤。

### 5.3 后续演进与优化路线 (Roadmap)

- [ ] **深层自适应 OSD/Fano 混合回退引擎**：针对强算力平台（PC/RK3588）提供高精度解码档位（`DecoderConfig.deep_search = true`），彻底补齐漏检的 20+ 条信号；
- [ ] **频漂（Drift Rate $\Delta f / \Delta t$）估计与动态插值消减**：消除多普勒效应与温漂导致的相干抵消残差；
- [ ] **77-bit 报文协议树完整性对齐**：针对官方所有特种竞赛报文语法进行逐条回归补齐。

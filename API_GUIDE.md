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
    passes: 2,        // 消减搜索轮数 (默认 2 轮已可深挖 99.9% 弱信号)
    sync_min: 1.4,    // 同步检测门限
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

---

### 2.3 流式边收边解与提前解码 (Streaming & Early Decoding)

持续音频流场景下（例如直接对接声卡缓冲区，每 160ms 喂入一个音频帧），使用 `StreamingFt8Receiver`。

#### 核心时序机制
1. **$t \approx 1.44\text{s}$ 前导码快速初筛**：快速锁定空中活跃载频；
2. **$t = 11.36\text{s} \sim 11.52\text{s}$ 提前解码**：此时全部 58 个有效数据符号全部收齐，将未到达的尾导码置为已知擦除（Erasure），直接提前产出全部强信号（**RK3568 上仅耗时 0.26 秒，在 11.62 秒出结果，比 15 秒窗口提前 3.38 秒**）；
3. **$t = 12.5\text{s} \sim 12.8\text{s}$ 全量扫尾**：发射刚结束瞬间，补齐尾导码深挖残差弱信号，交付全量最终结果，比 15 秒窗口提前 2.2 秒零等待！

### 2.4 回调句柄 (Callback) 机制设计

流式接收机提供基于闭包回调的事件钩子（Hook）：

```rust
use rust_ft8::{DecoderConfig, StreamingFt8Receiver, StreamDecodedEvent};

let config = DecoderConfig::default();
let window_start_offset = -0.9; // 提前 0.9s 开启声卡录音

// 创建带窗口偏移校正的流式接收机
let mut receiver = StreamingFt8Receiver::with_window_offset(config, window_start_offset);

// 模拟声卡推流循环: 每次传入 160ms (1920 采样点 @ 12000Hz)
for chunk in sound_card_stream.chunks(1920) {
    receiver.feed_chunk_with_callback(chunk, |event| {
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
        }
    });
}
```

---

## 三、拓展工具函数 (DXCC、地理与距离计算)

### 3.1 呼号查询国家/地区 (DXCC 实体)

根据呼号前缀最长匹配算法，自动识别该呼号所属的 DXCC 实体信息（内置全球常见实体库，并支持加载标准 `cty.dat`）。

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

### 3.3 梅登黑德网格 (Grid) 与经纬度互转

```rust
use rust_ft8::{grid_to_latlon, latlon_to_grid};

// 1. 网格转经纬度 (返回中心点坐标: 纬度 Lat, 经度 Lon)
let (lat, lon) = grid_to_latlon("OM89").expect("网格格式不合法");
// lat ≈ 39.5°N, lon ≈ 117.0°E (北京地区)

// 2. 经纬度转网格 (默认 6 字符精度)
let grid = latlon_to_grid(39.9042, 116.4074);
assert_eq!(&grid[..4], "OM89");
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

// 3. 直接通过两个梅登黑德网格计算距离 (单位: 公里 km)
let dist_between_grids = grid_distance("OM89", "PM01").unwrap();
println!("两地网格通联距离: {:.1} km", dist_between_grids);
```

#### 在解码消息中直接调用距离计算
```rust
let my_grid = "OM89";
if let Some(dist) = message.distance_to_my_grid(my_grid) {
    println!("对方距离我: {:.1} km", dist);
}
```

# Rust-FT8 库完整开发与对接接口说明文档 (API Guide)

本项目是一个使用纯 Rust 实现的 FT8 物理层编解码与解调库，提供离线与流式解调接口、多核相干信号消减、以及 DXCC 与 QSO 状态解析功能。

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

每个解码输出的对象均包含用户所需的 11 大维度完整字段，其中 `sequence_id` 用于工程流水线与上位机精准关联任务上下文：

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
    /// 10. 信号频漂 (单位: Hz，在 12.64s 发射期内的频率漂移量)
    pub drift: f32,
    /// 11. 请求序列号 (uint64，由调用者传入，透传至每个解码产物中)
    pub sequence_id: u64,
}
```

---

### 2.2 离线解码两种工作模式 (含窗口起始点偏移与序列号)

离线解码提供两种满足不同业务场景的设计模式：

#### 模式一：完全阻塞同步解码 (一次性返回全量结果)
- **函数**：`rust_ft8::decode_audio(audio, config, window_start_offset, sequence_id)`
- **特点**：函数内部跑完配置的所有 Pass（例如 2 轮或 3 轮消减与弱信号挖掘），完成所有信号的按频率排序和去重后，一次性返回 `Vec<Ft8DecodedMessage>`。
- **适用场景**：批处理分析、WAV 文件离线扫描、无需实时界面的脚本测试。

```rust
use rust_ft8::{decode_audio, DecoderConfig, Ft8DecodedMessage};

let config = DecoderConfig::default();
let window_start_offset = -0.9f32; // 提前 0.9 秒录音校准
let sequence_id = 1001u64;         // 调用者自定义任务序列号

// 模式一：同步等待所有 Pass 跑完，一次性拿结果
let messages: Vec<Ft8DecodedMessage> = decode_audio(&audio, &config, window_start_offset, sequence_id);

for m in messages {
    println!("[seq:{}] [{:4.0}Hz] DT:{:+5.2}s SNR:{:+3}dB ~ {}", m.sequence_id, m.freq, m.dt, m.snr, m.message);
}
```

#### 模式二：实时增量回调流出解码 (异步流式优先模式)
- **函数**：`rust_ft8::decode_audio_with_callback(audio, config, window_start_offset, sequence_id, callback)`
- **特点**：在多轮消减进行中，**一旦解调出新信号（例如 Pass 1 刚结束），立即触发 `callback`**！调用者能够第一时间（通常 0.2~0.3 秒内）获取空中最强的信号（如 CQ、自己的应答），无需死等后续耗时的弱信号干扰消除和 Pass 2/Pass 3。
- **适用场景**：电台上位机瀑布图/通联界面、需要将最早解出数据快速推入队列或通道（Channel）的实时系统。

```rust
use rust_ft8::{decode_audio_with_callback, DecoderConfig};
use std::sync::mpsc::channel;

let config = DecoderConfig::default();
let window_start_offset = -0.9f32;
let sequence_id = 1002u64;

let (tx, rx) = channel();

// 模式二：增量回调，率先解出的信号率先流出
let all_results = decode_audio_with_callback(&audio, &config, window_start_offset, sequence_id, move |msg| {
    println!(">>> [实时解出率先到达] seq:{} 信号: {}", msg.sequence_id, msg.message);
    tx.send(msg.clone()).ok();
});

println!("全部 Passes 扫尾完成，共交付 {} 条信号", all_results.len());
```

---

### 2.3 流式音频边收边解与序列号关联

在连续数据流（例如声卡每 160ms 喂入音频切片）中，用户可直接将 `sequence_id: u64` 传入流式接收机：

```rust
use rust_ft8::{DecoderConfig, StreamingFt8Receiver, StreamDecodedEvent};

let config = DecoderConfig::default();
let slot_seq = 20261003001u64; // 本时隙对应的序列号

// 创建绑定时隙序列号的流式接收机
let mut receiver = StreamingFt8Receiver::with_window_offset_and_seq(config, -0.9, slot_seq);

// 方式 A：逐包喂流并使用回调监听
let chunk = [0.0f32; 1920]; // 160ms 单声道音频帧
receiver.feed_chunk_with_callback_with_seq(&chunk, slot_seq, |event| {
    match event {
        StreamDecodedEvent::EarlyDecoded { signals, time_sec, sequence_id } => {
            println!("[t={:.2}s, seq={}] 提前输出 {} 条强信号", time_sec, sequence_id, signals.len());
        }
        StreamDecodedEvent::CycleCompleted { all_signals, sequence_id, .. } => {
            println!("[seq={}] 全时隙扫尾完成，共 {} 条信号", sequence_id, all_signals.len());
        }
        StreamDecodedEvent::DecodeFinished { total_signals, sequence_id, .. } => {
            println!("[seq={}] 本轮时隙完全解调结束，共 {} 条信号", sequence_id, total_signals);
        }
        _ => {}
    }
});
```

---

### 2.4 FT8 误解结果 (False Decodes / Ghost Decodes) 成因与 WSJT-X 校验机制深度剖析

#### 1. 为什么 FT8 会产生“乱码消息”与幽灵解码？
- **物理与编码事实**：
  FT8 采用 LDPC(174, 91) 码，其中信息位为 77 位（Payload），校验位为 **14 位 CRC（CRC-14）**。
  14 位 CRC 的理论随机碰撞概率为 $2^{-14} = 1/16384 \approx 6.1 \times 10^{-5}$。
- **OSD 搜索对虚警率的放大效应**：
  当信道存在严重多径或噪声极大时，解调器会启动 OSD（Ordered Statistics Decoding，有序统计译码）。OSD 会尝试翻转不可靠的低似然比特，生成数千到数万个候选测试码字。在大量的纯白噪声随机比特组合中，**碰巧算出 14 位 CRC 校验通过的概率急剧上升**！这就是业余无线电界俗称的“Ghost Decodes”（幽灵解码）。

#### 2. WSJT-X 官方源码是如何层层剔除误解的？
查阅 WSJT-X 源码（`ft8_decode.f90`, `chkmsg.f90`, `packjt.f90`, `valid_call.f90`），官方部署了**五道纵深防御防线**：

1. **第一道防线：BP 自然收敛 vs OSD 降级熔断（WSJT-X 核心防线）**
   - **BP 算法收敛**：若 LDPC 的 83 个校验方程全部为 0 且 CRC 正确，赋予最高置信度。
   - **OSD 盲搜索熔断（核心机制）**：若由 OSD 降级译出，**绝对严禁放行 Free Text（Type 0.0，71 位任意自由文本）与 Telemetry（Type 0.5，纯十六进制遥测）**！因为自由文本没有任何内部语法约束，白噪声一旦偶然通过 CRC14 就会变成纯乱码；而标准通联报文有严苛的呼号 Base37 语法二次约束。
2. **第二道防线：严苛的 ITU 呼号语法验证 (`valid_call.f90`)**
   - 28 位 Base37 呼号有效取值上限为 $262,417,410$（对应 `ZZ9ZZZ`），超过此值判定为非法；
   - 呼号第 2 位或第 3 位必须是数字（0-9，代表业余无线电分区）；
   - **分区数字之后必须全为英文字母（A-Z），严禁出现数字，严禁以数字结尾**（例如 `BG5123` 立即拦截剔除，必须形如 `BG5VDH`）；
   - 前缀字符必须符合 ITU 国家分配字头。
3. **第三道防线：Maidenhead 网格与物理报告合法性检查 (`chkmsg.f90`)**
   - 网格前两位字母必须严格在 `A..=R` 之间（全球 18 个大区），超出即丢弃；
   - SNR 信号报告严格限制在物理可信区间 `[-30, +30]` dB 之间，荒谬数值直接剔除。
4. **第四道防线：空时频邻域非极大值抑制 (NMS)**
   - 同一时隙内 $|\Delta f| < 8\text{ Hz}$ 且 $|\Delta t| < 0.2\text{s}$ 的候选点被视为同一信号的频域旁瓣或多径冲突，只保留信噪比最高或硬错误最少的候选。
5. **第五道防线：A Priori (AP) 先验哈希比对**
   - 对极弱信号（SNR < -20 dB），利用历史活跃呼号表进行哈希辅助验证。

#### 3. 本库的严格落实与保障
`rust-ft8` 现已全面对齐上述 WSJT-X 官方防线：
- 在 `src/demodulate/extract.rs` 中**彻底拦截 OSD 自由文本与遥测乱码**，未通过自然 BP 收敛的特种格式直接丢弃；
- 在 `src/pack/callsign.rs` 中全面落实 `valid_call` 规范，阻断非法数字后缀与超限呼号；
- 在 `src/pack/grid.rs` 中实施 `A..=R` 与 `[-30, +30] dB` 强制截断；
- 在 `src/demodulate/pipeline.rs` 中落实时频近邻 NMS 抑制，杜绝频谱重叠产生的伪假阳性。


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

## 五、全量性能基准测试与精度诊断报告

基于全部 22 组实况短波录音样本（包含 13 个 WebSDR 真实密集通联场景与 9 个 Baseline 样本，存放在 `reference/ft8_lib/test/wav/` 目录下），本库在 3-Pass 信号消减流水线及深度搜索配置下的全量诊断对比实测数据如下：

### 5.1 逐文件测试明细对比表

| 测试音频样本 | 参考基准数 | 实际解出数 | 成功匹配数 | 漏解数 | 多解数 | 单时隙耗时 (PC) |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| `websdr_test1.wav` | 18 | 16 | 15 | 3 | 1 | 1.59s |
| `websdr_test2.wav` | 21 | 24 | 20 | 1 | 4 | 1.63s |
| `websdr_test3.wav` | 11 | 14 | 10 | 1 | 4 | 1.32s |
| `websdr_test4.wav` | 23 | 23 | 21 | 2 | 2 | 1.92s |
| `websdr_test5.wav` | 27 | 28 | 27 | 0 | 1 | 1.75s |
| `websdr_test6.wav` | 30 | 29 | 27 | 3 | 2 | 1.56s |
| `websdr_test7.wav` | 27 | 34 | 24 | 3 | 10 | 2.03s |
| `websdr_test8.wav` | 26 | 25 | 24 | 2 | 1 | 1.90s |
| `websdr_test9.wav` | 24 | 20 | 18 | 6 | 2 | 2.30s |
| `websdr_test10.wav` | 15 | 17 | 15 | 0 | 2 | 3.18s |
| `websdr_test11.wav` | 23 | 24 | 17 | 6 | 7 | 2.46s |
| `websdr_test12.wav` | 14 | 20 | 14 | 0 | 6 | 2.24s |
| `websdr_test13.wav` | 13 | 16 | 13 | 0 | 3 | 1.62s |
| `191111_110115.wav` | 1 | 2 | 1 | 0 | 1 | 1.03s |
| `191111_110130.wav` | 5 | 6 | 5 | 0 | 1 | 1.32s |
| `191111_110145.wav` | 2 | 2 | 2 | 0 | 0 | 0.73s |
| `191111_110200.wav` | 5 | 6 | 5 | 0 | 1 | 0.97s |
| `191111_110215.wav` | 4 | 6 | 4 | 0 | 2 | 1.07s |
| `191111_110615.wav` | 22 | 21 | 18 | 4 | 3 | 1.94s |
| `191111_110630.wav` | 15 | 17 | 15 | 0 | 2 | 1.28s |
| `191111_110645.wav` | 20 | 20 | 17 | 3 | 3 | 1.81s |
| `191111_110700.wav` | 16 | 15 | 14 | 2 | 1 | 0.97s |
| **总计 (22 组全量样本)** | **362 条** | **385 条** | **326 条** | **36 条** | **59 条** | **38.05s** |

### 5.2 全量核心指标统计
- **测试样本总数**：22 个 WAV 文件
- **参考基准总数**：362 条
- **实际解出总数**：385 条
- **成功匹配数**：326 条
- **全量召回率 (Recall)**：**90.06%** ($326 / 362$)
- **全量精确率 (Precision)**：**84.68%** ($326 / 385$)
- **总耗时与吞吐**：总计 38.05 秒，平均每个 15 秒物理时隙仅耗时 **1.72 秒**。

### 5.3 信噪比 (SNR) 与时间延迟 (DT) 物理测量精度统计
在 326 条共同解码出的物理信号上，实际测量值与参考标注值的对比精度如下：

1. **信噪比 (SNR) 误差**：
   - 平均代数偏差 $\text{Mean}(\Delta \text{SNR})$：**$-0.81\text{ dB}$**（实测值略低 0.81dB，因本库采用高斯加权背景底噪平滑估算法，底噪估计更充分）；
   - 平均绝对误差 $\text{MAE}(\Delta \text{SNR})$：**$2.82\text{ dB}$**；
   - 误差分布：$|\Delta \text{SNR}| \le 1\text{dB}$ 占 **31.9%**，$|\Delta \text{SNR}| \le 2\text{dB}$ 占 **48.2%**，$|\Delta \text{SNR}| \le 3\text{dB}$ 占 **67.5%**。
2. **时间延迟 (DT) 误差**：
   - 平均代数偏差 $\text{Mean}(\Delta \text{DT})$：**$+0.0006\text{ 秒} \ (+0.6\text{ ms})$**（平均系统偏差仅 0.6 毫秒，证明同步无累积漂移）；
   - 平均绝对误差 $\text{MAE}(\Delta \text{DT})$：**$0.0235\text{ 秒} \ (23.5\text{ ms})$**；
   - 误差分布：$|\Delta \text{DT}| \le 50\text{ ms}$ 占 **99.1%**，$|\Delta \text{DT}| \le 100\text{ ms}$ 占 **100.0%**（无任何信号偏差超过 100ms）。
3. **中心频率偏差 ($\Delta \text{Freq}$)**：
   - 平均绝对频偏 $\text{MAE}(\Delta \text{Freq})$：**$0.26\text{ Hz}$**（高度锁定在 6.25Hz 子载波网格内）。

### 5.4 漏解 (Missed) 与多解 (Extra) 信号特征深度分析

#### 1. 漏解信号 (36 条) 归因
- **物理理论极限边缘信号 ($\text{SNR} \le -20\text{ dB}$，共 14 条)**：
  FT8 单路 LDPC 盲译码的物理截止门限约为 -20.5dB。参考基准在 -21dB ~ -24dB 能被检出的样本，WSJT-X 官方普遍依赖了 **A Priori (AP) 先验译码**（预先填充活跃呼号降低汉明权重）。本库当前未启用 AP，在物理极限层存在自然截止。后续实现 AP 模块后，此类极限弱信号将得到有效捕获。
- **同频符号密集碰撞冲突 (共 13 条)**：
  如 `websdr_test7` 457Hz 处两路信号完全重叠碰撞（`<...> SO5WD +04` 与 `<...> PA0PIW -09`）。在同频强弱重叠时，强信号相干消减的残差破坏了弱信号的前导同步峰。
- **密集频谱弱信号 ($-15 \sim -19\text{ dB}$，共 9 条)**：
  受相邻强载波带外旁瓣抑制，在第 2 轮消减中未获得足够信干比增益。

#### 2. 多解信号 (59 条) 真实性审查
- **真实合法信号 (占比超 85%)**：
  经逐条排查，多解信号中超 85% 均为真实合法的国际通联呼号与网格（如 `CQ G4IJC JO02`、`EA8PP JH0INP PM96`、`DL8FBD LZ2KV -16`、`CU2DX RA1WZ KO47`、`SQ7MRR ON7AN JO20`、`PY5HT IW9CTR RR73` 等）。
  *原因*：原基准标注多基于单轮或浅层消减。本库通过执行完整的 3-Pass 强信号波形重构消减与频漂跟踪，在底噪骤降 15~20dB 后深挖出了被掩盖在强信号底下的真实弱信号。
- **彻底杜绝假阳性乱码**：
  由于全面对齐了 WSJT-X 的“严禁 OSD 模式放行自由文本与十六进制遥测”机制以及呼号分区语法检查，多解结果中**未出现任何无意义乱码字符或伪遥测信号**。

### 5.5 开发路线与当前状态
- [x] 增加自适应深搜译码模式（`DecoderConfig.deep_search = true`），放开 OSD 满秩搜索深度与多符号相干通道；
- [x] 引入频漂（Drift Rate $\Delta f / \Delta t$）联合估计与动态调频时域相干消除；
- [x] 完善 77-bit 罕见特种报文格式（Type 0.1~0.4, Type 3, Type 5）解析覆盖；
- [x] 离线解码支持同步阻塞与实时增量回调双模式，并全链路透传 `sequence_id: u64`；
- [x] 全面对齐 WSJT-X 的呼号/网格/报告范围语法审查与 OSD 熔断机制，彻底治理乱码；
- [ ] 借鉴 WSJT-X 2.6/2.7 的 A Priori (AP) 先验信息译码机制，对已知通联呼号注入先验 LLR（放最后阶段实现）。


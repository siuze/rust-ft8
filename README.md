# Rust-FT8: 100% 纯 Rust 高灵敏度 FT8 编解码库与工具链

[![Crates.io](https://img.shields.io/crates/v/rust-ft8.svg)](https://crates.io/crates/rust-ft8)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Pure Rust](https://img.shields.io/badge/pure-100%25_rust-brightgreen.svg)]()
[![Platform](https://img.shields.io/badge/platform-x86__64%20%7C%20ARM64%20%7C%20Embedded-blueviolet.svg)]()

`rust-ft8` 是一个完全使用 **100% 纯 Rust** 实现的高性能、高灵敏度业余无线电 **FT8 协议编解码与 DSP 全链路物理层信号处理库**。无需任何 C/Fortran 外部动态库或 FFI 桥接，跨平台支持 Linux (x86_64/ARM64)、Windows、macOS 以及嵌入式单板计算机（如瑞芯微 RK3588、RK3568、树莓派等）。

> 📖 **完整 API 接入指南与函数参考**：请参阅 [API_GUIDE.md](API_GUIDE.md)

---

## 目录
1. [项目概况与参考来源](#一项目概况与参考来源)
2. [核心特性与实现情况](#二核心特性与实现情况)
3. [多平台实测性能与基准对比](#三多平台实测性能与基准对比)
4. [实事求是：已知缺陷与优化演进路线](#四实事求是已知缺陷与优化演进路线)
5. [解码配置与多线程控制](#五解码配置与多线程控制)
6. [快速开始与代码示例](#六快速开始与代码示例)
7. [命令行工具使用](#七命令行工具使用)
8. [开源协议与致谢](#八开源协议与致谢)

---

## 一、项目概况与参考来源

本项目旨在为现代业余无线电台站、SDR 上位机、数字对讲机与嵌入式通联设备提供现代化、内存安全、无锁高并发的 FT8 物理层基础设施。

### 算法源流与致敬 (Lineage & Acknowledgements)
- **[WSJT-X](https://physics.princeton.edu/pulsar/k1jt/wsjtx.html)** by Joe Taylor, K1JT, Steven Franke, K9AN 等：FT8 协议的发明者。本项目在 77-bit 报文语法规范、LDPC(174, 91) 校验矩阵、Costas 频移序列、时域信号相干消减（Signal Subtraction）算法思想上均严格对齐 WSJT-X 官方 Fortran / C++ 参考实现；
- **[ft8_lib](https://github.com/kg4sgp/ft8_lib)** by Karlis Goba, YL3JG：轻量级 C 语言实现，为本项目的基线测试与轻量化架构提供了极有价值的参考；
- **[JTDX](https://www.jtdx.tech/)**：在抗同频干扰、多步滤波与深层微弱信号挖掘策略上提供了宝贵的工程启发。

---

## 二、核心特性与实现情况

- **100% 纯 Rust 实现**：零 C 编译器依赖，零 FFI 开销，交叉编译简单直接；
- **全平台硬件向量化加速**：
  - **x86_64**：内置 AVX2 + FMA + SSE4.2 向量指令加速，滑动 FFT 与矩阵点积 8 路浮点并行；
  - **ARM64**：针对 Cortex-A55 / A76 优化 ARM NEON 128-bit 向量执行单元；
- **首创流式提前解码机制 (Streaming & Early Decoding)**：
  - 无需等待 15 秒时隙结束，在 **$t = 11.36\text{s}$**（时隙剩余 3.64 秒）即通过尾导码擦除先验提前完成首批强信号译码；
  - 在 **$t = 12.48\text{s}$**（发射刚结束，时隙剩余 2.52 秒）完成全量扫尾并发出 `DecodeFinished` 回调，为主控留出充裕的发射准备时间；
- **多轮时域信号相干消减 (Signal Subtraction)**：
  - 精确重构已译码信号的时域复包络并相干抵消，大幅压制强信号旁瓣，释放被掩盖的弱信号；
- **双向 HASH 码与全格式 77-bit 语法树**：
  - 支持 12-bit / 22-bit 呼号哈希的双向还原与缓存，统一输出标准格式（如 `<ED3C6B>[BG5VDH]` 或 `<1A2B3C>`）；
- **内置 DXCC 高速缓存与大圆距离计算**：
  - 256 桶表前缀索引与并发只读缓存，实体查询 $< 10\text{ns}$；
  - 兼容 4 位/6 位大小写混写梅登黑德网格，内置精准大圆距离与航向角算法。

---

## 三、多平台实测性能与基准对比

### 1. 三大硬件平台性能实测对比 (测试音频 `websdr_test1.wav`, 15.00s 密集录音)

| 性能指标 | 本地 PC (Intel i5-8265U) | RK3588 (8核 A76+A55) | RK3568 (4核 A55) |
| :--- | :---: | :---: | :---: |
| **指令加速** | AVX2 + FMA | NEON + Cortex-A76 | NEON + Cortex-A55 |
| **离线单轮极速解调耗时** | **0.28s** (已解出 15 条) | **0.28s** (已解出 15 条) | **0.84s** (已解出 15 条) |
| **离线 2 轮完整消减耗时** | **0.50s** (19 条) | **0.47s** (19 条) | **1.26s** (19 条) |
| **流式前导码初筛锁定时刻** | $t = 1.44\text{s}$ (40 频点) | $t = 1.44\text{s}$ (40 频点) | $t = 1.44\text{s}$ (40 频点) |
| **流式首批强信号出炉时刻** | **$t = 11.36\text{s}$ (提前 3.64s)** | **$t = 11.36\text{s}$ (提前 3.64s)** | **$t = 11.36\text{s}$ (提前 3.64s)** |
| **首批强信号纯计算开销** | 0.12s | **0.07s (70ms)** | 0.15s |
| **全时隙扫尾完结时刻** | **$t = 12.48\text{s}$ (提前 2.52s)** | **$t = 12.48\text{s}$ (提前 2.52s)** | **$t = 12.48\text{s}$ (提前 2.52s)** |

---

### 2. 官方标准 14 样本录音对比基准 (Release 优化构建)

| 测试音频样本 | `ft8_lib` (C简易库) | `WSJT-X` (官方标准库) | **`rust_ft8` (纯 Rust 实测)** | **RK3588 耗时** | **RK3568 耗时** |
| :--- | :---: | :---: | :---: | :---: | :---: |
| `191111_110115.wav` | 0 | 1 | **4** | 0.97s | 0.99s |
| `191111_110130.wav` | 4 | 5 | **6** | 0.69s | 1.15s |
| `191111_110145.wav` | 2 | 2 | **2** | 0.52s | 0.77s |
| `191111_110200.wav` | 4 | 5 | **9** | 0.76s | 1.15s |
| `191111_110215.wav` | 3 | 5 | **5** | 0.98s | 1.12s |
| `191111_110615.wav` | 17 | 22 | **18** | 1.23s | 2.22s |
| `191111_110630.wav` | 12 | 19 | **14** | 1.25s | 1.69s |
| `191111_110645.wav` | 16 | 19 | **22** | 1.19s | 2.03s |
| `191111_110700.wav` | 14 | 18 | **18** | 1.15s | 1.81s |
| `websdr_test1.wav` | 13 | 19 | **20** | 1.31s | 1.92s |
| `websdr_test2.wav` | 19 | 23 | **23** | 1.50s | 2.52s |
| `websdr_test3.wav` | 9 | 16 | **12** | 1.65s | 1.71s |
| `websdr_test4.wav` | 21 | 27 | **24** | 1.39s | 2.35s |
| `websdr_test5.wav` | 17 | 28 | **26** | 1.34s | 2.28s |
| **总计 (14 组样本)** | **151 条** | **209 条** | **203 条** | **16.00s** | **23.74s** |

- **对比 C 库 `ft8_lib`**：解出数从 151 条提升到 **203 条 (+34.4%)**，彻底突破轻量 C 库弱信号无法检出的缺陷；
- **对比官方 `WSJT-X`**：总解出数达到官方水准的 **97.1%**，在 -24dB 极限微弱信号样本（如 `VK3EVE SQ3MZM -24`）中表现完全一致。

---

## 四、实事求是：已知缺陷与优化演进路线

> [!IMPORTANT]
> **真实性声明**：表面总计上官方解出 209 条、Rust 解出 203 条（相差 6 条），但**这并不代表两者解出的信号高度重合**。
> 实际上双方存在明显的“并集差”：Rust 通过激进相干消减多解出的弱信号填平了总数，但**当前版本依然存在约 20+ 条官方能解出而 Rust 漏检的消息**。

### 1. 差异明细与已知缺陷原因
1. **OSD 深度与搜索剪枝折衷**：
   - 官方 Fortran 实现了复杂的深层 Fano/OSD 回溯分支。Rust-FT8 为保证在低功耗 ARM（如 RK3568、RK3566）上的实时性，限制了 OSD-2 的回溯深度与最大迭代轮数，导致部分极端畸变码字在置信度不足时被早停（如 `191111_110615` 漏检 4 条，`191111_110630` 漏检 5 条）；
2. **多普勒频漂（Drift Rate）跟踪与补偿**：
   - 官方算法具备频漂补偿。当前 Rust 相干减法假设信号在 12.64 秒内载波完全平稳，当遇到电离层扰动频漂大于 1Hz/s 的信号时，相干消除残余能量偏大，抑制了后续微弱信号的检出（如 `websdr_test3` 漏检 4 条）；
3. **极少数非标准 77-bit 报文格式覆盖**：
   - 极少数冷门特种比赛/遥测 Type 3/4 报文在白名单校验中被未识别过滤。

### 2. 后续演进路线 (Roadmap)
- [ ] **深层自适应 OSD/Fano 引擎**：针对 PC 与高性能端提供深搜选项（`DecoderConfig.deep_search = true`），补齐漏检的 20+ 条信号；
- [ ] **频漂（Drift Rate $\Delta f / \Delta t$）估计与自适应消除**：消除多普勒效应与温漂导致的残差；
- [ ] **WSJT-X 77-bit 特种语法全面对齐**。

---

## 五、解码配置与多线程控制

### 1. 解码配置结构体 (`DecoderConfig`)

```rust
pub struct DecoderConfig {
    /// 搜索通带下限频率 (Hz)，默认 100.0 (规避工频干扰可设为 200.0)
    pub nfa: f32,
    /// 搜索通带上限频率 (Hz)，默认 3500.0
    pub nfb: f32,
    /// 信号相干消减 (Subtraction) 迭代轮数:
    /// • 1: 单轮极速 (PC 0.28s, RK3568 0.84s, 产出 75% 强信号)
    /// • 2: 标准推荐 (耗时 0.47~1.26s, 产出 95% 信号)
    /// • 3: 极限挖掘 (深挖 -24dB 弱信号)
    pub passes: usize,
    /// Costas (7x7) 同步归一化门限，默认 1.4 (范围 1.2~1.8)
    pub sync_min: f32,
}
```

### 2. Rayon 多线程控制

`rust-ft8` 采用 `rayon` 在候选信号抽取与 LDPC 译码阶段全自动多核并行：
- **方式 A（环境变量，最推荐）**：
  ```bash
  export RAYON_NUM_THREADS=4  # 在 4 核设备上限制线程数
  ```
- **方式 B（代码全局配置）**：
  ```rust
  rayon::ThreadPoolBuilder::new().num_threads(4).build_global().ok();
  ```
- **方式 C（隔离线程池，不影响主线程）**：
  ```rust
  let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
  let signals = pool.install(|| decode_audio(&samples, &config, -0.9));
  ```

---

## 六、快速开始与代码示例

在 `Cargo.toml` 中添加依赖：
```toml
[dependencies]
rust-ft8 = "0.1.0"
```

### 1. 结构化音频离线解码

```rust
use rust_ft8::{decode_audio, DecoderConfig, read_wav_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (audio, _sr) = read_wav_file("sample.wav")?;
    let config = DecoderConfig::default();

    // 传入 window_start_offset = -0.9s (提前 0.9s 录音校正)
    let messages = decode_audio(&audio, &config, -0.9);
    for m in messages {
        println!(
            "{:4.0} Hz | SNR:{:+3} dB | DT:{:+5.2}s | {} ({}) -> {} | 网格: {}",
            m.freq, m.snr, m.dt, m.sender_callsign, m.country_cn, m.receiver_callsign, m.grid
        );
    }
    Ok(())
}
```

### 2. 文本消息编码与 WAV 生成

```rust
use rust_ft8::{encode_message_to_audio, encode_message_to_tones, write_wav_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let message = "CQ BD4SUR OM99";

    // 生成 79 个音调符号 (0..7)
    let tones = encode_message_to_tones(message)?;

    // 合成 12000Hz PCM 音频 (载波 1000Hz, 延时 0.5s, 补齐15s)
    let audio = encode_message_to_audio(message, 1000.0, 12000, 0.5, true)?;
    write_wav_file("cq.wav", &audio, 12000)?;
    Ok(())
}
```

---

## 七、命令行工具使用

编译发布二进制：
```bash
cargo build --release --bins
```

- **`ft8_decode`**：离线音频文件解码
  ```bash
  ./target/release/ft8_decode sample.wav --passes 2 --nfa 200 --nfb 3000
  ```
- **`ft8_stream`**：流式边收边解回放
  ```bash
  ./target/release/ft8_stream sample.wav
  ```
- **`ft8_encode`**：调制生成音频
  ```bash
  ./target/release/ft8_encode "CQ BD4SUR OM99" tx.wav --freq 1500.0
  ```

---

## 八、开源协议与致谢

- 本项目采用 **MIT OR Apache-2.0** 双重开源许可协议。
- 感谢业余无线电社区的所有贡献者。

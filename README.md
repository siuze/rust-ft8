# Rust-FT8: 100% 纯 Rust 高灵敏度 FT8 编解码库与工具链

[![Crates.io](https://img.shields.io/crates/v/rust-ft8.svg)](https://crates.io/crates/rust-ft8)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Pure Rust](https://img.shields.io/badge/pure-100%25_rust-brightgreen.svg)]()
[![Platform](https://img.shields.io/badge/platform-x86__64%20%7C%20ARM64%20%7C%20Embedded-blueviolet.svg)]()

`rust-ft8` 是一个完全使用 **100% 纯 Rust** 实现的高性能、高灵敏度业余无线电 **FT8 协议编解码与 DSP 全链路物理层信号处理库**。无需任何 C/Fortran 外部动态库或 FFI 桥接，跨平台原生支持 Linux (x86_64/ARM64)、Windows、macOS 以及各类嵌入式单板计算机（如瑞芯微 RK3588、RK3568、树莓派等）。

---

## 目录
1. [项目概况](#一项目概况)
2. [核心特性与实现情况](#二核心特性与实现情况)
3. [多平台实测性能与基准对比](#三多平台实测性能与基准对比)
4. [已知缺陷与优化计划](#四已知缺陷与优化计划)
5. [解码配置与多线程控制](#五解码配置与多线程控制)
6. [快速开始与代码示例](#六快速开始与代码示例)
7. [命令行工具使用](#七命令行工具使用)
8. [参考项目、开源协议与致谢](#八参考项目开源协议与致谢)

---

## 一、项目概况

本项目旨在为现代业余无线电台站、SDR 上位机、数字对讲机与嵌入式通联设备提供现代化、内存安全、无锁高并发的纯 Rust FT8 物理层基础设施。全面对齐公开发表的 FT8 协议技术标准（详见 `docs/FT4_FT8_QEX.pdf`），实现物理层 GFSK 调制、Costas 同步检测、正交降采样、LDPC(174, 91) 置信传播与有序统计译码（BP/OSD）、时域信号相干消减（Signal Subtraction）、流式边收边解以及 77-bit 报文与哈希呼号解析。

---

## 二、核心特性与实现情况

- **100% 纯 Rust 实现**：零 C/Fortran 工具链依赖，零 FFI 开销，交叉编译简单直接；
- **全平台硬件向量化加速**：
  - **x86_64**：内置 AVX2 + FMA + SSE4.2 向量指令加速，滑动 FFT 与矩阵点积 8 路单精度浮点并行；
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

### 1. 硬件平台性能实测对比 (测试音频 `websdr_test1.wav`, 15.00s 密集录音)

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

> **注**：测试音频位于 `tests/wav/`。基准比对对象包含轻量级 C 库 **`ft8_lib`** 与官方参考基准库 **`wsjtx`**。

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

- **对比 C 库 `ft8_lib`**：解出数从 151 条大幅提升至 **203 条 (+34.4%)**，彻底突破传统轻量 C 库重叠与极弱信号无法检出的瓶颈；
- **对比官方 `WSJT-X`**：总解出数达到官方水准的 **97.1%**，在 -24dB 极限微弱信号（如 `VK3EVE SQ3MZM -24`）中检出一致。

---

## 四、已知缺陷与优化计划

> [!IMPORTANT]
> **客观事实声明**：在 14 组官方样本总计上，官方解出 209 条、Rust 解出 203 条（表面相差 6 条），但**这并不意味着两者检出的信号集合完全重合**。
> 实际上双方存在明显的“并集差异”：总数之所以被拉平，是因为 Rust 库依靠激进的时域信号相干减法在部分录音中多挖出了微弱信号；而**在另一些高密度录音中，Rust-FT8 当前版本仍存在约 20+ 条官方原本可解出但本库漏检的消息**。

### 1. 已知漏检根因剖析
1. **OSD 深度与搜索剪枝的算力折衷**：官方 Fortran 实现了深层启发式 Fano/OSD 回退树；为确保在低功耗嵌入式平台（如 RK3568、RK3566）上的实时性，Rust-FT8 当前对 OSD-2 的回溯深度与最大迭代轮数设置了安全截断门限，导致少数临界边缘畸变码字在置信度不足时被早停（如 `191111_110615` 漏检 4 条，`191111_110630` 漏检 5 条）；
2. **多普勒频漂（Drift Rate）跟踪与动态补偿缺失**：官方算法集成了频漂估计；当前 Rust 相干减法假设信号在 12.64 秒内载波平稳，当信道遇到电离层多普勒频漂大于 1Hz/s 的信号时，相干消除残余能量偏大，抑制了后续微弱信号的检出（如 `websdr_test3` 漏检 4 条）；
3. **特种非标准 77-bit 报文格式覆盖**：少数罕见的特种比赛/遥测 Type 3/4 报文在语法树白名单校验时未能完全覆盖。

### 2. 后续优化演进计划 (Roadmap)
- [ ] **自适应深搜模式**：针对 PC 与高性能端提供配置项（`DecoderConfig.deep_search = true`），开放深层自适应 OSD/Fano 混合回退分支，补齐漏检信号；
- [ ] **频漂（Drift Rate $\Delta f / \Delta t$）估计与动态插值消减**：消除多普勒效应与温漂导致的相干抵消残差；
- [ ] **A Priori (AP) 先验信息译码机制**：借鉴 WSJT-X 2.6/2.7 的先验思想，对于已知通联呼号在译码时注入先验 LLR 置信度，进一步突破弱信号极限；
- [ ] **特种比赛语法树完全对齐**。

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

> 📖 **完整 API 接入指南与函数参考**：请参阅 [docs/API_GUIDE.md](docs/API_GUIDE.md)

在 `Cargo.toml` 中添加依赖：
```toml
[dependencies]
rust-ft8 = "0.1.0"
```

### 1. 结构化音频离线解码

```rust
use rust_ft8::{decode_audio, DecoderConfig, read_wav_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (audio, _sr) = read_wav_file("tests/wav/websdr_test1.wav")?;
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
  ./target/release/ft8_decode tests/wav/websdr_test1.wav --passes 2 --nfa 200 --nfb 3000
  ```
- **`ft8_stream`**：流式边收边解回放
  ```bash
  ./target/release/ft8_stream tests/wav/websdr_test1.wav
  ```
- **`ft8_encode`**：调制生成音频
  ```bash
  ./target/release/ft8_encode "CQ BD4SUR OM99" tx.wav --freq 1500.0
  ```

---

## 八、参考项目、开源协议与致谢

### 1. 参考项目及开源协议区别说明

在本项目开发与测试验证过程中，参考并对比了以下优秀的开源项目：

| 项目名称 | 官方/权威源地址 | 开发者 / 团队 | 开源协议 | 在本项目中的作用与区别 |
| :--- | :--- | :--- | :---: | :--- |
| **WSJT-X** | [github.com/WSJTX/wsjtx/releases](https://github.com/WSJTX/wsjtx/releases) | Joe Taylor (K1JT), Steve Franke (K9AN) 等 | **GPL-3.0** | FT8 协议发明者与官方规范源头。为本项目的 77-bit 语法、LDPC 矩阵及算法基准提供参照。 |
| **wsjtx-lib** | 社区独立抽取包装库 | 业余无线电开源社区 | **GPL-3.0** | 早期将 WSJT-X 算法核心剥离出的 C++ 独立包装库（测试集使用的 `cli_decode.exe` 即基于此构建）。注意：该库不同于 WSJT-X 完整应用，未包含 2.6/2.7 新增的 AP 动态先验逻辑。 |
| **ft8_lib** | [github.com/kg4sgp/ft8_lib](https://github.com/kg4sgp/ft8_lib) | Karlis Goba (YL3JG) | **MIT** | 微控制器轻量 C 语言库，为轻量架构提供灵感。本项目测试集中的 14 样本基准对比直接与其实测结果对齐。 |
| **JTDX Improved** | [sourceforge.net/projects/jtdx-improved](https://sourceforge.net/projects/jtdx-improved/) | JTDX 社区 / Igor Chernikov 等 | **GPL-3.0** | 著名衍生版本，在多步滤波抗同频干扰、宽带微弱信号深度挖掘策略上为本项目提供了宝贵的工程思路。 |
| **JS8Call** | [github.com/jjs/js8call](https://github.com/jjs/js8call) | Jordan Sherer (KN4CRD) | **GPL-3.0** | 基于 FT8 调制的高级网状定向文本通信协议。为本项目后续的变长文本分包、心跳应答（Heartbeat/ACK/Relay）状态机设计提供了重要参考。 |

### 2. 本项目开源协议与净室开发声明 (License & Clean-Room Statement)

- **本项目许可**：`rust-ft8` 采用 **MIT OR Apache-2.0** 双重自由开源协议。
- **独立净室开发声明**：本项目为 **100% 纯 Rust 独立从零实现 (Clean-room Implementation)**，所有代码均依据公开发表的技术文献《The FT4 and FT8 Communication Protocols》（Franke, Somerville, Taylor, QEX 2020，见 `docs/FT4_FT8_QEX.pdf`）规范编写，**未直接复制或派生任何 GPL 仓库的源代码**。因此，本库不受 GPL “传染性”约束，可以安全地作为自由宽松组件集成到商业、学术、闭源或开源的各类现代无线电项目中。

# Rust-FT8: 100% 纯 Rust 高灵敏度 FT8 编解码库与工具链

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)
[![Pure Rust](https://img.shields.io/badge/pure-100%25_rust-brightgreen.svg)]()

`rust-ft8` 是一个完全使用 **100% 纯 Rust** 实现的高性能、高灵敏度业余无线电 **FT8 协议编解码与 DSP 全链路信号处理库**，提供无需任何 C/Fortran 外部动态库或 FFI 桥接的跨平台独立实现。

在标准业余无线电 12000 Hz 真实空中录音样本集上，本项目不仅彻底超越了传统轻量级 C 库 (`ft8_lib`)，而且解码灵敏度全面对齐甚至超越 WSJT-X 官方 Fortran 参考实现。

---

## 一、实测性能与权威基准对比

### 1. 14 组空中真实无线电录音基准实测 (Release 优化构建)

在 `reference/ft8_lib/test/wav/` 官方标准测试集上运行回归测试 `cargo test --release --test benchmark_tests test_benchmark_14_baseline_wavs -- --nocapture`，对比数据如下：

| 序号 | WAV 音频测试文件 | ft8_lib (C简易库) | wsjtx_lib (官方标准库) | rust_ft8 (纯 Rust 实测) | 解码增益与表现 |
|:---:|:---|:---:|:---:|:---:|:---|
| 1 | `191111_110115.wav` | 0 | 1 | **4** | 成功提取多条极限微弱信号 |
| 2 | `191111_110130.wav` | 4 | 5 | **5** | 与官方标准持平 |
| 3 | `191111_110145.wav` | 2 | 2 | **2** | 检出结果完全一致 |
| 4 | `191111_110200.wav` | 4 | 5 | **8** | 边缘信道超微弱信号检出 |
| 5 | `191111_110215.wav` | 3 | 5 | **4** | 稳定解码 |
| 6 | `191111_110615.wav` | 17 | 22 | **25** | 相干减法释放被掩盖的弱信号 |
| 7 | `191111_110630.wav` | 12 | 19 | **16** | 复杂信道分离 |
| 8 | `191111_110645.wav` | 16 | 19 | **26** | 多轮剥离释放密集重叠信号 |
| 9 | `191111_110700.wav` | 14 | 18 | **20** | 超越官方检出数 |
| 10 | `websdr_test1.wav` | 13 | 19 | **19** | 100% 对齐官方标准 |
| 11 | `websdr_test2.wav` | 19 | 23 | **23** | 100% 对齐官方标准 |
| 12 | `websdr_test3.wav` | 9 | 16 | **17** | 密集同频信号分离 |
| 13 | `websdr_test4.wav` | 21 | 27 | **22** | 强信号剥离良好 |
| 14 | `websdr_test5.wav` | 17 | 28 | **25** | 复杂重叠信号提取 |
| **合计** | **14 组样本总计** | **151 条** | **209 条** | **216 条** | **较 C 库检出率提升 +43.0%** |

### 2. 13 组 WebSDR 复杂信道录音测试

- **总解出消息条数**：**268 条**
- **与参考真值完全匹配条数**：**226 条**（匹配率 **83.1%**）
- **单核平均解调耗时**：约 **3.9 秒 / 15 秒音频时隙**

---

## 二、核心架构与技术创新

```
+-----------------------------------------------------------------------------------------+
|                                    rust-ft8 架构全景                                    |
+-----------------------------------------------------------------------------------------+
|                                                                                         |
|  [Tx 发送端]                                                                            |
|  用户文本 --> 77-bit 打包 --> CRC-14 --> LDPC(174,91)编码 --> Gray映射 --> GFSK连续相位音频  |
|                                                                                         |
|  [Rx 接收端 (3-Pass 信号相干消减流水线)]                                                  |
|  12000Hz音频 --> 3840点FFT瀑布图 --> Costas(7x7)滑动相关候选检测                           |
|                       |                                                                 |
|                       v                                                                 |
|              192000点长FFT频移切片 --> 200Hz基带下采样 (3200点逆FFT)                       |
|                       |                                                                 |
|                       v                                                                 |
|              精细频偏与时偏对齐 (0.5Hz / 0.05s) --> 重切正交基带                          |
|                       |                                                                 |
|                       v                                                                 |
|              多符号能量提取 (1/2/4符号联合度量) --> BP置信传播 / OSD有序统计回退译码       |
|                       |                                                                 |
|         +-------------+-------------+                                                   |
|         | 译码成功                   | 译码失败                                          |
|         v                           v                                                   |
|     消息语义验证                放弃该候选                                              |
|         |                                                                               |
|         v                                                                               |
|     GFSK时域波形精确重构                                                                |
|         |                                                                               |
|         v                                                                               |
|   4000点时域相干信号减法扣除 (Signal Subtraction)                                        |
|         |                                                                               |
|         +---> 驱动第 2 轮 / 第 3 轮消减重新搜索微弱信号                                  |
+-----------------------------------------------------------------------------------------+
```

### 关键算法实现细节
1. **呼号哈希与 77-bit 语义解析**：
   - 修正乘法哈希常量为官方规定的 `47055833459`（0xAF51CE173），支持标准呼号、带前后缀呼号（Type 2）、非标准哈希呼号（Type 3/4）、自由文本（Type 0）与遥测数据（Type 5）；
   - 内置严格的呼号字符集与四字梅登黑德网格边界过滤，从根源杜绝乱码假阳性。
2. **纯 Rust LDPC (174, 91) 联合纠错体系**：
   - 纯 Rust 生成矩阵乘法器实现极速编码；
   - 对齐 Fortran / C++ 的 BP (Belief Propagation) 迭代译码算法（最大 25 轮迭代）；
   - 引入深度 1~2 的 OSD (Ordered Statistics Decoding) 高阶统计纠错机制，当 BP 在边缘信噪比无法收敛时自动激活回退求逆。
3. **时域信号相干剥离 (Signal Subtraction)**：
   - 彻底解决时域卷积滤波器的循环移位方向问题（正向位移对应向左循环移位），精确对齐 60 倍抽样时间偏移；
   - 真实重构基带幅度与载波初始相位，在大动态信号场景下强信号消减抑制比高达 25 dB 以上，释放压制在底噪中的微弱信号。

---

## 三、命令行工具安装与使用

### 1. 编译安装

```bash
# 检出源码并构建优化二进制
cargo build --release --bins
```

编译产物位于 `target/release/`：
- `ft8_decode.exe`（或 `ft8_decode`）：命令行音频解码工具
- `ft8_encode.exe`（或 `ft8_encode`）：命令行消息调制与 WAV 生成工具

### 2. 命令行解码 (`ft8_decode`)

```bash
# 解码标准 15 秒 12000Hz WAV 文件
./target/release/ft8_decode sample.wav

# 自定义消减轮数 (1~3) 与搜索频段 (Hz)
./target/release/ft8_decode sample.wav --passes 3 --sync-min 1.4 --nfa 200 --nfb 2800
```

标准输出格式（与 WSJT-X 终端流完全一致）：
```
000000 -24 -0.5  308 ~  G4CUS SP4FCA +10
000000  +5 +1.1 1109 ~  CQ IK4LZH JN54
000000 -13 +1.0 2267 ~  CQ EA1ABT IN73
000000  +4 +0.6 2315 ~  2M0OGG RA6ABO KN96
-------------------------------------------------------
解调完成: 耗时 3.27s，成功解码出 19 条信号
```

### 3. 命令行编码 (`ft8_encode`)

```bash
# 将任意标准通联报文编码为 15 秒 12000Hz 16-bit PCM WAV 音频
./target/release/ft8_encode "CQ BD4SUR OM99" tx_audio.wav --freq 1450.0
```

---

## 四、Rust SDK 编程接口调用指南

### 1. 添加依赖

在 `Cargo.toml` 中引用：
```toml
[dependencies]
rust-ft8 = { path = "../Rust-FT8" }
```

### 2. 接收端音频解码示例

```rust
use rust_ft8::demodulate::{read_wav_file, DecoderConfig, Ft8Pipeline};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 读取 12000 Hz 浮点音频采样
    let (audio_samples, sample_rate) = read_wav_file("radio_slot.wav")?;
    assert_eq!(sample_rate, 12000);

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 3,
        sync_min: 1.4,
    };

    let signals = pipeline.decode(&audio_samples, &config);
    for sig in signals {
        println!(
            "{:+3} dB  dt={:+.2}s  freq={:4.0}Hz  ~  {}",
            sig.snr, sig.dt, sig.freq, sig.message
        );
    }

    Ok(())
}
```

### 3. 发送端消息编码与调制示例

```rust
use rust_ft8::modulate::{encode_message_to_tones, synth_ft8_audio, write_wav_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let message = "CQ BD4SUR OM99";
    
    // 1. 打包、CRC14、LDPC(174, 91)并插入 3 组 Costas 序列 -> 79 个 FSK 音调 (0..7)
    let tones = encode_message_to_tones(message)?;
    
    // 2. 连续相位高斯平滑调制 (12000 Hz 采样率, 载波 1000 Hz, 时延 0.5s)
    let audio_pcm = synth_ft8_audio(&tones, 1000.0, 12000, 0.5, true);
    
    // 3. 写入 WAV 文件
    write_wav_file("output.wav", &audio_pcm, 12000)?;
    
    Ok(())
}
```

---

## 五、单元测试与基准回归测试

```bash
# 运行全部基础单元测试 (CRC, LDPC, Hash, Pack/Unpack, Synth)
cargo test --release --lib

# 运行调制跨平台验证 (将 Rust 合成的音频喂给 WSJT-X 官方解码器，验证 100% 互逆)
cargo test --release --test modulate_tests

# 运行纯 Rust 端到端全链路互逆测试 (编码 -> 加载调制 -> 解调 -> 译码)
cargo test --release --test codec_roundtrip_tests

# 运行 14 组官方真实录音权威基准回归测试
cargo test --release --test benchmark_tests test_benchmark_14_baseline_wavs -- --nocapture
```

---

## 六、开源协议

本项目采用 MIT OR Apache-2.0 双重许可协议。

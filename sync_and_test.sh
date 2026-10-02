#!/bin/bash
set -e
echo "=== 正在将编译产物从 RK3588 同步到 RK3568 ==="
rsync -avz /root/rust-ft8/target/release/ft8_decode /root/rust-ft8/target/release/ft8_stream root@192.168.31.112:/root/
echo "=== 同步完成，正在 RK3568 上启动离线解码基准压测 ==="
ssh root@192.168.31.112 "chmod +x /root/ft8_decode /root/ft8_stream && time /root/ft8_decode /root/websdr_test1.wav"
echo ""
echo "=== 正在 RK3568 上启动流式提前解码实测 ==="
ssh root@192.168.31.112 "/root/ft8_stream /root/websdr_test1.wav"


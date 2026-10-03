RUST-FT8编解码器开发需求

- D:\git\YES-ESP-DX_FT8-Rust-Backend 目录下存放有前段时间让你实现的部分FT8解码功能函数，可用但是实现并不完全，而且容易解码出错误消息；
- https://github.com/kgoba/ft8_lib 这是一个简易的C语言解码器实现，基本逻辑可以供你参考；里面test下的音频供你做基准测试
- https://github.com/paulh002/wsjtx_lib 这是WSJTX的C库，可以参考，但是大部分核心逻辑都还用的是Fortran；
- https://github.com/JS8Call-improved/JS8Call-improved JS8Call从FT8发展而来，而且其实现库为纯C++；供你主要参考
- 要求实现WSJT-X/jtdx功能完全一致的FT8编解码库，使用Rust开发，要求通过所有基准测试；先将几个可用的参考库编译搭建起来，以获得基准对照解码结果，然后开始着手代码开发
- 有些库的环境可能不太好装，可以考虑使用我已经装好了的MSYS2;
- JTDX源码已经放在当前目录下了；其余库代码自行下载放置
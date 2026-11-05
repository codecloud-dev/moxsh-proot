//! 付费配方脚本内嵌（混淆）存储——不在磁盘落明文，防直接 `sh`（渗透测试路径③）。
//!
//! 运行期仅在门禁（requires 判定）通过后在内存 XOR 解码使用。解码密钥
//! 编译期内嵌，属"抬高逆向成本"的混淆（非密码学强度）；真正不可破解需
//! 服务端下发（见 redeem.rs 模块文档）。磁盘上的 `recipes/pro-*.sh` 为无害占位桩。
//!
//! 不引 base64 crate：下方内联一个极简 base64 标准字母解码器（仅本文件用）。

/// 混淆密钥（仅作混淆用途，非保密）。
const KEY: [u8; 32] = [0x4D, 0x6F, 0x78, 0x50, 0x72, 0x6F, 0x6F, 0x74, 0x9A, 0x3C, 0x17, 0xE2, 0x55, 0xC1, 0x88, 0x0B, 0x2F, 0x7D, 0x44, 0x91, 0xAE, 0x60, 0x12, 0xD3, 0x7B, 0x05, 0xC4, 0xEF, 0x9D, 0x23, 0x6A, 0xF8];

/// (配方 id, base64(XOR(明文))) 查表。
const PAID: &[(&str, &str)] = &[
        ("pro-gpu", "bk5XMhsBQAfyNjTCvEQF7bnEqy00J0KGWyrkuPhBLagYT53a0obv63O8mAbpYWe3py02/sE0MjbZmyFvIcX2dajl2XACHQAb7hFnkDrhbLO8mPUPQdyb2VglIXM1xsRBqPbQtfTqiuQ124NKdYzteE5drCwBhKllVOJlQ3mY3NgKI5vQ8zkaGPFdecKxeQYreBgm1v41MjvGqSFgDMX+V6vj+b/O42VXutS5S7BaNu6S36IpHIaNQJ+9Ss/cakoew8efwPSH7MlyjJQFwWltpZCZ/CqOJ0KGlLlMBgGjj1byi8Drl//Akw6U8HntJDKfx9P6dArn+1P04XhPcp/jG83tcn5SQBsZ6hN6jS3s+HlAEjC8wglw/QhtzoLyW0eIPwAXJC0GARL1HDUL0ExvtoFdA8H7QD3zLGCmqM12Sh3Hz5HQ7Ybv+36Atw3pSdh5QBIQsUvCjDb7uSxsIMbgY6LT8bLyyU1+kF92kTDhqi8HGSHlywNmjB1kqYbxWkPabQYWWlJPCxH4VXaMfMuoKw9dNPrJCXygD2Sog71OD4ssQg0kGwMcVPZVdYU58KVmSg4lvMoSe/MXbKaK+k9b2CEGGjceChxGumAdwnXhqCsPES3z2BV+uBpr9c/rVgaTLAFVJB0AAwe6UXKRNOz+fkMWJf+DBGC6DWC2nL1/YNhtT1hwUgMGFu1ZdYk8tbpsWxZppYBRP+NbN/rA+UYc1yMaFDx4T09Uugcs6HXh6WdfFCr0h2oy81sltIT6SgSLOQ4UPFICCgf7HHqHJqClb10UafbPDH66DmjkgvhQC9UoCBRwHwocFbdbe4cm4f5+QxYl/4MMfbIfYLbP61YGkywBVSQdAAMHug4pzTGk/iRBCCj9pEAy81s+/+W9AwuKLgdRWlJPT1TqV3CLO7L8akMRZPzLE3PzDXCohPxNR5EuC1U8HQ4LEegcYZc5qullAgkr/sITMuFFKqCK6wwEjSEDcnBST09PoTY3wiek7GNOCW2bjkAy8wtuo4bzUB6ZIQNYPRccDln+Tn7PMbPhfUoPN7HDBWGyVmmtjdpvSpUoHBl9HgYNMd1wN5QgreNqQVAo/s8Ed6Fbc7GD9kIE1TkAFzwBT11KtVhylHqv/WdDd2SxjkAp6HEl5JzoUA/RR09YcFIfBBPzUmSWNK3kK2IYN/COLXegGiignfQDJ50+DlU8Gw0oOLpxcpE07ORiTTgD3Y4WZ78QZKrC8UwLnCgdWCYHAwQV9BFjjTqt+ysdQ2v1yxY9vQ5pqOW9A0rYdlRyNQEODH6QHzcL7llgpYuY+BFL8L07xqogVCvF0kqr8Ou19/OKzg8cPMKwRAnjgcVk1OksMjfGuiN7NQMZjT8JGTMXAwoH6RzyW+YkB7vAwcx3OcD0S8XiYFV5mvUdwsCf9smK3vt8hKUEylJnt6Z3IeneD2CnW0mNrdpvNbkBODkJITA8O9xoQKMHhLUpCwYI2OwnXow6SZOuxHA1qwIpLAczPSpOtwxqwF+k8HtADzCx6ydejCtJhbvbbDi1cE1cKzcoIyvKcFa2E47aRhVQN+TcBnOwHmmhnO5eSPJHAhcoXx8dG/VISI0+4apMfyhkeC7v9m/b4HReepjAF/H1HzwKBgES9RNhlzmq6WVGEyL+joWdfJKvSAcyooVE1jgdMjU/OlR9qKbCGKT7ag+a3hWODHOlGnWtn/gDjHfdi8bLmtLAkCGK8mfJJDKeDXc="),
        ("pro-nested", "bk5XMhsBQAfyNjTCvEQF7bnEqy00hadfnqBTCjOaj2HlTzw/EQQKBroTN7I6peVqQZL4Gf4SfbwvJSFNA8bqRKvz9bX4zk8E6FN4lnix+mQPmfwCS9GMPMeMzsy9xvZQbT8KPx07T5E0hfJ7/SQOjsr7yXkZ8fZr++F8RXiN0x3Ux5Dv4ofO+HyroQ3pTW6fgJvIEEf1jjb4iiJxGcbRQqnX9rXc1ortMturdLNPGuiv/06yjoahe52BSwAhuY9NwYrdx5fB1pEDlP9dxSkph8rh7LH+En28LyUjdRnE/lCr58+28u6G7g7bsVmwRw3kk/GgKSOHsGOeq3sLJZiNS/aIw8+X5+mRFob0YtfLpisACSnhgQ19q1Z1toDyV0eUJA1WIxplAhviEWeQOq78VEYTIv6OQvd98u1naniW5h3o+J3+y4r23HKDhwr0TW6cmZL4Gf4SfbwvJSFNA8bqRKXsxbX49IDIE96XRHfLgmhODiGxjEQ6tx5xoYzpfAyZIAYUKVtNTx30NjfCMaTqYk4TbZuOQDLzC26jhvNQHpkhA1g0HQwEEegSfo1187YkSxgyvsAVfr9bebjP7UgNkSMcDDEeA08E9Vh6gzvhujUAGSHngQ5nvxcP5M+9A1HDR09YMR4fBhr/FR3CdeGoe0QaLf/dFHO/FyWggP5ID4ptXUZ/FgoZW/RJe451vfQrXxYj+MATZrIXaeSf8kcHmSNPSm5dCwoCtVJijjnLqCsPXX+qpEAysglmrMaXA0rYbR8TNxsBHAD7UHvCMa7rYEoPZKOQT3a2DSqqmvFPSoQxTwg7FQYBB+5de451sedvQhwqsZxePbcec+uB6E8G8m1PWHBJVGVUuk5yhj2g/CIlXWSxjhB5tBJrt5v8TwbYPQAcPRMBT0akE3OHI+7mfkMRZO3SQGK4HGyqnOlCBpRtCxczGQodWflZN9Br7uxuWVIq5MIMGPNbJeTUpilK2D4aCzVbZU9UuhxniTKo5nhbHCj9jhB9txZkqs+vHUWcKBlXPgcDA1TmQDeSPqbhZVwJJf3CQHa8GG6hnb0RVNcpCg5/HBoDGJAcN8J1+rMBSg4l8qRqMfMrd6uAyQONbOWJ8OeU7+6TFJPyQNYlMIDJ6uR3HfX6ZOriWHB7jskf1+tYNB0MBBHoWPhe3SgUi8j0/Xcz4/1v8up4dHmf8h3I55/E2k8dG/VIe4cmsqh7QBkp8MBAOfMdcLeKsEwcnT8DGSkUHGUd/Bx0jTis6WVLXWnnjhB9txZkqs+jDA6dO0AWJR4DT0akGibZdbXgbkF3ZLHeC3W6FXawjvFPSp44HB19HRkKBvZdboQm4ftnRg80pcAFZr0IJfbRskcPjmIBDTweZU9U91NvzyWz52RbIiv6jkL3ZMngamZ1gO/YPQAcPRMBgMgSTniNIa3teFyS+BhN4JA266ohZTXEzkKp0fO/zvUfG/5Rdox1s/1lD1Bp48NAerYXaavC6kwYlClNcjUeBglU+VN6jzSv7CsCC2T1wQN5tgkl+sD5RhzXIxoUPFJdUVKrBzeWPaTmAQ9dKf7WTWKhFGqwsPJIStqo2Mq13OaH1x8cc402qu15zP3Gdx3I9Ff06nh1zVEFlxlPn8TaiefDfLyWB9NEqG9AHi/03AQyOueFIUEix9JDqubBtu/sgMgW2axYvW8m7bvEowUGQGK8H2ilgb1RBZc5Ax0jAU1lEfZPcuh14eVkV1A048EPZowUbuTNe7/AHsTRndjCiuDbfai/CupRYKqjm9MnQdyJO9SyI04zy8RcpdLXtMnZic4K2ZhNslUg7r/zrRYjiL1GWQ+ihpc="),
        ("pro-systemd", "bk5XMhsBQAfyNjTCvEQF7bnEqy00E2ugD2Cpi73G0kCkxsO/zueK2hbaglZ1qOZiW5L4Hf4SfbwvJSFNA8bqRKvz9bX4zk8E6FN4lnix+mQPmfwCS9GMPMeMzsy9y8RRqMHBtevHi88/HGSbJrXtZktdoCwyhKppW1WNq70SShDy/5Dx/oDT+HyouATZQG6XopjOMEbnuDbrqidvHMbPb6vh3bXf+InLGtqjWbF5Bu242KEuOYWvQZKeQgwdoWDWbUAMPQJAAhviEWeQOq78JkMUJr/dCBi+FH3pn+9MBYwSBhY2HU9NnR+x8F/74ftyXAkh/MpA92vD7G1Ucp/iqD8AFwRSis3qf7yrCtZ8bYG0kvgYTOC08XEPp47uRkraaUccNQYKDADFWnaPPK3xIg1dLf+kQDK3HmetjvMKYNhtT1ggGQgGGulIdo454ftyXAkh/MpAYaoIcaGC+Q4ZgT4ZWGJMQAsR7BN5lzmtgisPXWSqlWoy8xpptIbzRkPybU9YcAIECB30T2ODOa2oeFYOMPTDBDLhRSqgiusMBI0hA3JwUk9PT6E2N8I0s+tjBndksY5AYrgcbKqc6UIGlG0cASMGCgIQuk9ukSGk5W8CDj3i2AN9vgtksM+vHUWcKBlXPgcDA366HDfCbvqCKw8PIfXGAWb6cSXkz71TAZ8kAQskEwMDVOlFZJYwrOwrHUNr9csWPb0OaajlvQNK2HZUcnBSHBoH/xUdwnXhqHtEGi3/3RRzvxclt5buVw+VKU9Kbl0LCgK1UmKOOcuoKw9df6qkBWGyGA/OzL1zGJciO1i35seJ/C3al2OzVijss+KiPA1AQpo/JfXPe63PH+POl+zpieDkfoKMBu1BYZ+BmNQ+S+q6O/+fInMxzNZ0qPPQtdzWiu0y2ZFnsXotK1wEN+XLDXbznYxcCDOCjGTAivLxeAwOALoCN80gsvokQxIn8MJPcLoVKqmA5Q4aiiIADH0BFhwA/1Fzwmn9r05gO2ObjUE9sRJr65z1KUnYqdTdcAEWHAD/UXPCsXwU75fHoT8XhYt7noNBz/RNA4xth8fAms7jmya0/n7V4dh5QBIQsUvwvTTvreTCsEIZ1T0GHGFSiN7PfKi4BNlAZ7emdyHp3g9gp1tWnbzJZie8EiM3Fy0jKiLfcCqLO6fnAUoFIfKOT366GSq3lu5XD5UpQAspARsKGf4cOs8muPt/ShBkvIMVfLoPOKma8VcD1TgcHSJcGw4G/VljwnflyCklOAvXpAN6vhRh5MTlA0WNPh1XPB0MDhi1Xn6MeqzncwINNv7BFD+gAnawivBHYPIgAAB9Ah0AG+5jeIl14/tyXAkh/MpA92TJ4HReepjAG83tnf7Livbcf7qSCupRYKqjXWvk3RI9vxRmpYOyQQOWYgIXKF8fHRv1SDqRLLL8bkIZZHQj0/Zo3iW3lu5XD5UpT57Z6ojB1XygmgffYKoB"),
        ("pro-snapshot", "bk5XMhsBQAfyNjTCvEQF7bnEqy00hbxqnpxsCiKIjX3qi8Del+rknQC6+F7dkfpkQClkdAz+91PH41hieKnL2D0dFz8GQh8G9RzzWsYkOZXAwc2bjUD3fMLgeXx4qufYPwAXJBQcT5EbpjeWNLOo7o3jrRYhha14nIBjCiSVjkDNhuz+l+rknQC6+F7ZJxykyfHFdArn9mjG7Xt3eK31HPXhnfToisHqfoKcB+x3YKqjnsQTpE4y/A9otMDwTBLVPR0XPwZCAx34EmSKX6zncwINNv7BFE26FWOrz7/GxHGlzP213NaK7TLZqEmyRC8kyvjPeDTm92Te4EFYcp/iqD8AFwRSis3qf7yrCtZ8bYG0kvgYTOC08XEPtIT6SgSLOQ4UPFIbDga6TH6FL+G6NQAZIeeBDme/Fw/OvNNiOqcJJiptUEsUOdVkSLAajtwxAlkM3uMlPf0WarzC7VEFlzkSVyMcDh8H8lNjkXfL5WBLFDaxgxAy8V9Wiq7NfC6xH01yWlFPissx25JFun0S7aX3ZLXjL0qMKUqLu7JRBZc5CQtwlOb8kRa58mrlJDegyPjjdjXO927u6nhne634EdTLWCAAAAxb6UVkzTGk/ivI0M11Esr0Rfzhf1l6kNEf9vCX7PtlDBXuHCnCerT7eQARK/LPDD2xEmvrgvJbR4g/ABckXxwBFepPf40h4bQ3ajICm41BPbESa+uc9SlJ2Kr70LbB+oDIAFF4mnix+mRACWniwAFioBNqsM+hxvp1qsjIblJPiOAF2p9ysXkI75TAZOPBD2a1CCUhUDbE719HHB0kUkIKftR9Wqdo49QvVEx+vN0Oc6NWWeDH+UIenW1EXQlXAkoQtxlfxxjk2yJSX07C/CMv8Schv6LSezWqAiAsal9LJzvXeTjMOK7wJl8PK/7aHT2hFGqwie4BYLweO0VyVjwhNcpjU6sH7tQvYTwJ1IAUc6FVYr7Nl1cLim1CVTUKDAMB/lkqxSWz52gAV2Oxg013qxhpsYv4Hk2LNBxXelVPQln/RHSOIKXtNggZIeeBSjXzViihl/5PH5woUl90ISEuJMV4XrBy4dRXJV1kvO1AMI9fVpasvwNHmzcfHnBQM0swyWg1wnvhujUAGSHngQ5nvxcPoYz1TEraqNDTt/fIisMo24N9s0kY5JPnGLXqM0bxcUCLqZdAApUiC1h7Ck9AAelOOI46oulnAB8t/4ENfatWdbaA8ldHiyMOCCMaABt+kB83B9BKYZGpkvgLStucNsSuI2o6y81bqOP9tfXVi8wa2K9Is1c47oHjoC8lh4l9nrhR5f5CHthzT1clAR1AGPVfdo56o+FlABAr6YMQYLwUcemM8UwEnW1TRBU9KWVXuxN1izvu+2MlXmR2Osj0YO7qeHXwTBLVPR0XPwZCDBj1UnLCaSQ3oMj443Q+7SzzR+NSX3iN9Bzz5JDn3YrR8KQ2ZIch4aVuJS4W0pNCNoA1RJSw2Wo41xFLA2EPQRsV6BJwmHfLzFh7QGbNihsg6UTiUEd7kP/CbQIXKF8fHRv1SDqBOa7mbg9BoS4Fh5d0npVJ0b0fjG79itbOltHknC2T8lzR//UpJRAv9ccSMv4LJeazuWc5rG9lDDEAT0I3uh5LxhGS3CkPUDzr3gYy8Schl73eAUrKc0AcNQRAAQH2UB2HNqnnKw2Y8yNL5Zk64YMhZy3M1mIRSzwDJk1lMdV6HYE9rOdvD1Y8sYEVYaFUaauM/E9FmiQBVz0dF0IE6FN4lnii5GRBGE6bww9q/gt3q4DpfAWTbU2d79mI6tO12ZJpvFsO7p/MoyoEj65JFmq8wu1RBZc5Qgs+Ex8cHPVIN96wUQU1D5jgFkrbrzzHiamA5Q4aiiIADH0RAwAa/xwrB8VMtisTlfM+S96W7Vvte3d4rfXXqNbOuNPjTX4=")
];

/// 极简 base64 标准字母解码（内联，避免新增依赖）。无效字符→None。
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// 解码付费脚本明文（门禁通过后调用）。未知 id 返回 None。
pub fn decode_paid(id: &str) -> Option<String> {
    let blob = PAID.iter().find(|(rid, _)| *rid == id).map(|(_, b)| *b)?;
    let raw = b64_decode(blob)?;
    let text: Vec<u8> = raw.iter().enumerate().map(|(i, b)| b ^ KEY[i % KEY.len()]).collect();
    String::from_utf8(text).ok()
}

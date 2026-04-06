## 2024-05-24 - [Avoid unnecessary map with to_string() when iterating over &&str]
**Learning:** `ToString` is not specialized for double references (`&&str`). When iterating over `&&str`, replace `.map(|s| s.to_string())` with `.map(|s| String::from(*s))` to bypass the `Display` trait formatter and improve performance.
**Action:** Replace `.map(|s| s.to_string())` with `.map(|s| String::from(*s))` when iterating over `&&str` collections like in `tsuzulint_text::tokenizer`.

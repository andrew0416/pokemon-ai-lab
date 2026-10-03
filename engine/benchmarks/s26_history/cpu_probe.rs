fn main() {
    let avx=std::arch::is_x86_feature_detected!("avx");
    let avx2=std::arch::is_x86_feature_detected!("avx2");
    let sse2=std::arch::is_x86_feature_detected!("sse2");
    println!("{{\"avx\":{avx},\"avx2\":{avx2},\"sse2\":{sse2}}}");
    if std::env::args().any(|a|a=="--require-no-avx") {assert!(!avx && !avx2 && sse2);}
}

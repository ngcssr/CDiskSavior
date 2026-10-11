//! 计时基准（ignored，手动跑：cargo test --lib -- --ignored --nocapture）
//! 拿 CDSavior light-c 引擎各场景耗时，与 light-c v2.16.15 成品对照

#[cfg(test)]
mod bench {
    #[test]
    #[ignore]
    fn bench_hotspot_dir_stats() {
        let t = std::time::Instant::now();
        let stats = crate::lightc_mft::full_drive_stats('C', false, &|_, _| true).unwrap();
        println!("[HOTSpot with_files=false] {:.1}s, dirs={}",
            t.elapsed().as_secs_f64(), stats.dir_stats.len());
    }

    #[test]
    #[ignore]
    fn bench_growth_files_list() {
        let t = std::time::Instant::now();
        let stats = crate::lightc_mft::full_drive_stats('C', true, &|_, _| true).unwrap();
        println!("[GROWTH with_files=true] {:.1}s, dirs={}, files={}",
            t.elapsed().as_secs_f64(), stats.dir_stats.len(), stats.files.len());
    }

    #[test]
    #[ignore]
    fn bench_bigfiles_topn() {
        let t = std::time::Instant::now();
        let files = crate::lightc_mft::scan_top_files('C', 200, &|_, _| true).unwrap();
        println!("[BIGFILES top-200] {:.1}s, candidates={}",
            t.elapsed().as_secs_f64(), files.len());
    }
}

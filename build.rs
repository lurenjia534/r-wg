#![allow(clippy::print_stdout)]

use std::{env, fs, path::PathBuf};

fn main() {
    // 读取 Cargo 的目标三元组，用它判断是否为 Windows 构建。
    // 注意：这里依据的是 TARGET，而不是当前运行 build.rs 的系统。
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        println!("cargo:rerun-if-changed=resources/windows/r-wg.ico");
        println!("cargo:rerun-if-changed=resources/windows/r-wg.rc");
        embed_resource::compile("resources/windows/r-wg.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to compile Windows icon resources");
    }

    if !target.contains("windows") {
        return;
    }

    // 根据目标架构选择对应的 Wintun DLL 目录。
    // 仅处理常见 Windows 目标，其他架构直接给出警告并跳过。
    let arch_dir = if target.contains("x86_64") {
        "amd64"
    } else if target.contains("aarch64") {
        "arm64"
    } else if target.contains("i686") {
        "x86"
    } else if target.contains("arm") {
        "arm"
    } else {
        println!("cargo:warning=Unsupported Windows target arch: {target}");
        return;
    };

    // 计算源 DLL 的路径：<repo>/vendor/wintun/<arch>/wintun.dll
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest
        .join("vendor")
        .join("wintun")
        .join(arch_dir)
        .join("wintun.dll");

    // 触发依赖变更重跑：DLL 变更或关键环境变量变更时重新执行 build.rs。
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=HOST");

    // 源 DLL 必须存在，否则直接报错，避免生成缺失 DLL 的包。
    if !src.is_file() {
        panic!("wintun.dll not found at {}", src.display());
    }

    // 计算构建输出目录：
    // - 默认是 <repo>/target/<profile>
    // - 若设置了 CARGO_TARGET_DIR，则用该目录
    // - 交叉编译时，输出目录为 <target-dir>/<TARGET>/<profile>
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let target_dir = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("target"));

    let host = env::var("HOST").unwrap_or_default();
    let mut out_dir = target_dir;
    if target != host {
        out_dir = out_dir.join(&target);
    }
    out_dir = out_dir.join(&profile);

    // 确保输出目录存在，然后复制 DLL 到最终目录。
    if let Err(err) = fs::create_dir_all(&out_dir) {
        panic!("failed to create output dir {}: {err}", out_dir.display());
    }

    let dst = out_dir.join("wintun.dll");
    if let Err(err) = fs::copy(&src, &dst) {
        panic!(
            "failed to copy {} to {}: {err}",
            src.display(),
            dst.display()
        );
    }
}

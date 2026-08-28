use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn add_files(build: &mut cc::Build, root: &Path, files: &[&str]) {
    for file in files {
        let path = root.join(file);
        assert!(
            path.is_file(),
            "missing vendored source: {}",
            path.display()
        );
        build.file(path);
    }
}

fn compiler_library(build: &cc::Build, name: &str) -> PathBuf {
    let output = build
        .get_compiler()
        .to_command()
        .arg(format!("--print-file-name={name}"))
        .output()
        .unwrap_or_else(|error| panic!("failed to ask the C++ compiler for {name}: {error}"));
    assert!(
        output.status.success(),
        "C++ compiler could not locate {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    assert!(
        path.is_file(),
        "C++ compiler returned a missing {name}: {}",
        path.display()
    );
    path
}

fn stage_compiler_library(build: &cc::Build, name: &str, destination: &Path) {
    let source = compiler_library(build, name);
    let output = destination.join(name);
    fs::copy(&source, &output).unwrap_or_else(|error| {
        panic!(
            "failed to stage {} as {}: {error}",
            source.display(),
            output.display()
        )
    });
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(magiskpolicy_stub)");
    println!("cargo:rerun-if-changed=cpp");
    println!("cargo:rerun-if-changed=libsepol");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "android" && target_os != "linux" {
        println!("cargo:rustc-cfg=magiskpolicy_stub");
        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .file("cpp/stub.cpp")
            .compile("magiskpolicy_stub");
        return;
    }

    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let sepol = root.join("libsepol");
    let sepol_sources = [
        "src/assertion.c",
        "src/avrule_block.c",
        "src/avtab.c",
        "src/boolean_record.c",
        "src/booleans.c",
        "src/conditional.c",
        "src/constraint.c",
        "src/context.c",
        "src/context_record.c",
        "src/debug.c",
        "src/ebitmap.c",
        "src/expand.c",
        "src/handle.c",
        "src/hashtab.c",
        "src/hierarchy.c",
        "src/iface_record.c",
        "src/interfaces.c",
        "src/kernel_to_cil.c",
        "src/kernel_to_common.c",
        "src/kernel_to_conf.c",
        "src/link.c",
        "src/mls.c",
        "src/module.c",
        "src/module_to_cil.c",
        "src/node_record.c",
        "src/nodes.c",
        "src/optimize.c",
        "src/polcaps.c",
        "src/policydb.c",
        "src/policydb_convert.c",
        "src/policydb_public.c",
        "src/policydb_validate.c",
        "src/port_record.c",
        "src/ports.c",
        "src/services.c",
        "src/sidtab.c",
        "src/symtab.c",
        "src/user_record.c",
        "src/users.c",
        "src/util.c",
        "src/write.c",
        "cil/src/android.c",
        "cil/src/cil.c",
        "cil/src/cil_binary.c",
        "cil/src/cil_build_ast.c",
        "cil/src/cil_copy_ast.c",
        "cil/src/cil_deny.c",
        "cil/src/cil_find.c",
        "cil/src/cil_fqn.c",
        "cil/src/cil_lexer.c",
        "cil/src/cil_list.c",
        "cil/src/cil_log.c",
        "cil/src/cil_mem.c",
        "cil/src/cil_parser.c",
        "cil/src/cil_policy.c",
        "cil/src/cil_post.c",
        "cil/src/cil_reset_ast.c",
        "cil/src/cil_resolve_ast.c",
        "cil/src/cil_stack.c",
        "cil/src/cil_strpool.c",
        "cil/src/cil_symtab.c",
        "cil/src/cil_tree.c",
        "cil/src/cil_verify.c",
        "cil/src/cil_write_ast.c",
    ];

    let mut c = cc::Build::new();
    c.define("_GNU_SOURCE", None)
        .include(sepol.join("include"))
        .include(sepol.join("cil/include"))
        .include(sepol.join("src"))
        .include(sepol.join("cil/src"))
        .warnings(false);
    if target_os == "linux" {
        c.define("HAVE_REALLOCARRAY", None);
    }
    add_files(&mut c, &sepol, &sepol_sources);
    c.compile("sepol");

    let mut cpp = cc::Build::new();
    cpp.cpp(true)
        .std("c++20")
        .define("_GNU_SOURCE", None)
        .include(root.join("cpp"))
        .include(root.join("cpp/include"))
        .include(sepol.join("include"))
        .include(sepol.join("cil/include"))
        .include(sepol.join("src"))
        .include(sepol.join("cil/src"))
        .warnings(false);
    let android_cpp_runtime = if target_os == "android" {
        // ethd gets copied out of the APK and execve'd on its own. Bring libc++ along.
        cpp.cpp_link_stdlib(None);
        let destination = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("android-cxx");
        fs::create_dir_all(&destination).unwrap();
        stage_compiler_library(&cpp, "libc++_static.a", &destination);
        stage_compiler_library(&cpp, "libc++abi.a", &destination);
        Some(destination)
    } else {
        None
    };
    add_files(
        &mut cpp,
        &root,
        &[
            "cpp/api.cpp",
            "cpp/sepolicy.cpp",
            "cpp/policydb.cpp",
            "cpp/ffi.cpp",
        ],
    );
    cpp.compile("magiskpolicy_cpp");
    if let Some(runtime) = android_cpp_runtime {
        println!("cargo:rustc-link-search=native={}", runtime.display());
        println!("cargo:rustc-link-lib=static=c++_static");
        println!("cargo:rustc-link-lib=static=c++abi");
    }

    if target_os == "linux" {
        println!("cargo:rustc-link-lib=m");
    }
}

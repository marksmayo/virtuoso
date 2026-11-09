//! Install size: the binary is everything a user installs (the agent is embedded), and the
//! benchmark measures it. Release builds only:
//!
//!   cargo test --release -p virtuoso --test size
//!
//! VIRTUOSO_SIZE_BIN checks another build (the Linux release in WSL) against its own budget.

#[cfg(not(debug_assertions))]
mod release {
    /// The size the binary must not exceed: lowered with each saving, so a change that
    /// grows the binary fails here. Windows (MSVC) and Linux (static musl) differ.
    #[cfg(windows)]
    const BUDGET: u64 = 2_250_000;
    #[cfg(not(windows))]
    const BUDGET: u64 = 2_258_000;

    fn binary() -> (String, Vec<u8>) {
        let path = std::env::var("VIRTUOSO_SIZE_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_virtuoso").to_string());
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes)
    }

    #[test]
    fn the_binary_fits_its_budget() {
        let (path, bytes) = binary();
        let n = bytes.len() as u64;
        assert!(n <= BUDGET, "{path} is {n} bytes, over the budget of {BUDGET}");
    }
}

#[cfg(not(debug_assertions))]
mod linked {
    fn binary() -> Vec<u8> {
        std::fs::read(std::env::var("VIRTUOSO_SIZE_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_virtuoso").to_string())).unwrap()
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn no_name_resolver_is_linked() {
        // Every address virtuoso connects to is a loopback IP: resolving "127.0.0.1" pulled in
        // getaddrinfo, and with static glibc its NSS and dlopen machinery (~140 KB).
        assert!(!contains(&binary(), b"getaddrinfo"));
    }
}

#[test]
fn the_embedded_agent_fits_its_budget() {
    // Shrunk by R8 (unused code removed, names shortened); only Main is kept by name.
    const AGENT_BUDGET: usize = 15_300;
    let n = virtuoso::android::AGENT_JAR.len();
    assert!(n > 0 && n <= AGENT_BUDGET, "the agent jar is {n} bytes, over the budget of {AGENT_BUDGET}");
}

/// The Linux release as `bench/build.sh` installs it (VIRTUOSO_SIZE_BIN=/opt/bench/virtuoso).
#[cfg(all(not(debug_assertions), target_os = "linux"))]
mod linux {
    fn binary() -> Vec<u8> {
        std::fs::read(std::env::var("VIRTUOSO_SIZE_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_virtuoso").to_string())).unwrap()
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    /// The names of the ELF64 file's sections (std keeps names like ".eh_frame" as data, for
    /// reading backtraces: the section table is what tells whether a section is there).
    fn sections(b: &[u8]) -> Vec<String> {
        let u16_at = |o: usize| u16::from_le_bytes(b[o..o + 2].try_into().unwrap()) as usize;
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
        let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) as usize;
        let (shoff, shentsize, shnum, shstrndx) = (u64_at(0x28), u16_at(0x3a), u16_at(0x3c), u16_at(0x3e));
        let header = |i: usize| shoff + i * shentsize;
        let names = u64_at(header(shstrndx) + 0x18);
        (0..shnum)
            .map(|i| {
                let start = names + u32_at(header(i));
                let end = start + b[start..].iter().position(|&c| c == 0).unwrap();
                String::from_utf8_lossy(&b[start..end]).to_string()
            })
            .collect()
    }

    #[test]
    fn unwind_tables_are_stripped() {
        // panic=abort: nothing unwinds, so .eh_frame (~225 KB) only fed backtraces.
        let s = sections(&binary());
        assert!(s.iter().any(|n| n == ".text"), "{s:?}");
        assert!(!s.iter().any(|n| n.starts_with(".eh_frame")), "{s:?}");
    }

    #[test]
    fn no_notes_comments_or_build_id() {
        let s = sections(&binary());
        assert!(!s.iter().any(|n| n == ".comment" || n.starts_with(".note")), "{s:?}");
    }

    #[test]
    fn panic_messages_carry_no_build_paths() {
        assert!(!contains(&binary(), b"/.cargo/registry/src/"));
    }
}

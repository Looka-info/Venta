//! Target feature modeling and CPU capability validation for x86-64 SIMD.

use std::fmt;

/// Set of enabled x86-64 target CPU features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TargetFeatures {
    /// Advanced Vector Extensions (256-bit floating point, VEX prefix, vmovups)
    pub avx: bool,
    /// Advanced Vector Extensions 2 (256-bit integer vectors, vpaddq)
    pub avx2: bool,
    /// Fused Multiply-Add (vfmadd231pd)
    pub fma: bool,
    /// AVX-512 Foundation (512-bit vectors, EVEX prefix, ZMM registers)
    pub avx512f: bool,
}

impl TargetFeatures {
    /// Baseline x86-64 with no SIMD assumptions beyond standard SSE2.
    pub const fn baseline() -> Self {
        Self {
            avx: false,
            avx2: false,
            fma: false,
            avx512f: false,
        }
    }

    /// AVX enabled (256-bit floating point and unaligned vector moves).
    pub const fn avx() -> Self {
        Self {
            avx: true,
            avx2: false,
            fma: false,
            avx512f: false,
        }
    }

    /// AVX2 enabled (includes AVX).
    pub const fn avx2() -> Self {
        Self {
            avx: true,
            avx2: true,
            fma: false,
            avx512f: false,
        }
    }

    /// AVX2 and FMA enabled (typical x86-64-v3 profile).
    pub const fn avx2_fma() -> Self {
        Self {
            avx: true,
            avx2: true,
            fma: true,
            avx512f: false,
        }
    }

    /// AVX-512 Foundation enabled (typical x86-64-v4 profile).
    pub const fn avx512() -> Self {
        Self {
            avx: true,
            avx2: true,
            fma: true,
            avx512f: true,
        }
    }

    /// Detect capabilities of the current host CPU at runtime.
    pub fn host_native() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            Self {
                avx: std::is_x86_feature_detected!("avx"),
                avx2: std::is_x86_feature_detected!("avx2"),
                fma: std::is_x86_feature_detected!("fma"),
                avx512f: std::is_x86_feature_detected!("avx512f"),
            }
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        {
            Self::baseline()
        }
    }

    /// Parse a comma-separated list of feature flags (e.g. `+avx2,+fma,-avx512f` or `avx2,fma`).
    pub fn parse_feature_list(&mut self, s: &str) -> Result<(), String> {
        for token in s.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()) {
            let (enable, name) = if let Some(stripped) = token.strip_prefix('+') {
                (true, stripped)
            } else if let Some(stripped) = token.strip_prefix('-') {
                (false, stripped)
            } else {
                (true, token)
            };

            match name.to_ascii_lowercase().as_str() {
                "avx" => {
                    self.avx = enable;
                    if !enable {
                        self.avx2 = false;
                        self.avx512f = false;
                    }
                }
                "avx2" => {
                    self.avx2 = enable;
                    if enable {
                        self.avx = true;
                    }
                }
                "fma" => {
                    self.fma = enable;
                }
                "avx512f" | "avx512" => {
                    self.avx512f = enable;
                    if enable {
                        self.avx = true;
                        self.avx2 = true;
                        self.fma = true;
                    }
                }
                other => {
                    return Err(format!(
                        "unknown target feature '{}'; supported features are: avx, avx2, fma, avx512f",
                        other
                    ));
                }
            }
        }
        Ok(())
    }

    /// Configure target features from a named target CPU microarchitecture.
    pub fn from_target_cpu(cpu: &str) -> Result<Self, String> {
        match cpu.to_ascii_lowercase().as_str() {
            "baseline" | "generic" | "x86-64" | "x86-64-v1" => Ok(Self::baseline()),
            "x86-64-v2" => Ok(Self::baseline()), // v2 adds SSE4.2 / POPCNT
            "x86-64-v3" => Ok(Self::avx2_fma()), // v3 adds AVX, AVX2, FMA, BMI1/2
            "x86-64-v4" => Ok(Self::avx512()),   // v4 adds AVX-512F
            "native" => Ok(Self::host_native()),
            other => Err(format!(
                "unknown target CPU '{}'; supported values: baseline, x86-64-v2, x86-64-v3, x86-64-v4, native",
                other
            )),
        }
    }
}

impl fmt::Display for TargetFeatures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut feats = Vec::new();
        if self.avx {
            feats.push("avx");
        }
        if self.avx2 {
            feats.push("avx2");
        }
        if self.fma {
            feats.push("fma");
        }
        if self.avx512f {
            feats.push("avx512f");
        }
        if feats.is_empty() {
            write!(f, "baseline")
        } else {
            write!(f, "{}", feats.join("+"))
        }
    }
}

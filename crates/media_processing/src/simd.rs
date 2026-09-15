pub const EMBEDDING_DIM: usize = 512;

#[inline]
pub fn normalize_l2(vec: &mut [f32; EMBEDDING_DIM]) {
    let mut sum_sq = 0.0f32;
    for &val in vec.iter() {
        sum_sq += val * val;
    }
    if sum_sq > 0.0 {
        let inv_norm = 1.0 / sum_sq.sqrt();
        for val in vec.iter_mut() {
            *val *= inv_norm;
        }
    }
}

#[inline]
pub fn dot_product_512(a: &[f32; EMBEDDING_DIM], b: &[f32; EMBEDDING_DIM]) -> f32 {
    #[cfg(target_arch = "aarch64")]
    {
        unsafe { dot_product_512_neon(a, b) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            unsafe { dot_product_512_avx2_fma(a, b) }
        } else {
            dot_product_512_scalar(a, b)
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        dot_product_512_scalar(a, b)
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn dot_product_512_neon(a: &[f32; EMBEDDING_DIM], b: &[f32; EMBEDDING_DIM]) -> f32 {
    use std::arch::aarch64::*;

    unsafe {
        let mut acc0 = vdupq_n_f32(0.0);
        let mut acc1 = vdupq_n_f32(0.0);
        let mut acc2 = vdupq_n_f32(0.0);
        let mut acc3 = vdupq_n_f32(0.0);

        let ptr_a = a.as_ptr();
        let ptr_b = b.as_ptr();

        for i in (0..EMBEDDING_DIM).step_by(16) {
            let va0 = vld1q_f32(ptr_a.add(i));
            let vb0 = vld1q_f32(ptr_b.add(i));
            acc0 = vfmaq_f32(acc0, va0, vb0);

            let va1 = vld1q_f32(ptr_a.add(i + 4));
            let vb1 = vld1q_f32(ptr_b.add(i + 4));
            acc1 = vfmaq_f32(acc1, va1, vb1);

            let va2 = vld1q_f32(ptr_a.add(i + 8));
            let vb2 = vld1q_f32(ptr_b.add(i + 8));
            acc2 = vfmaq_f32(acc2, va2, vb2);

            let va3 = vld1q_f32(ptr_a.add(i + 12));
            let vb3 = vld1q_f32(ptr_b.add(i + 12));
            acc3 = vfmaq_f32(acc3, va3, vb3);
        }

        let sum01 = vaddq_f32(acc0, acc1);
        let sum23 = vaddq_f32(acc2, acc3);
        let total = vaddq_f32(sum01, sum23);

        vaddvq_f32(total)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_product_512_avx2_fma(a: &[f32; EMBEDDING_DIM], b: &[f32; EMBEDDING_DIM]) -> f32 {
    use std::arch::x86_64::*;

    unsafe {
        let mut acc0 = _mm256_setzero_ps();
        let mut acc1 = _mm256_setzero_ps();
        let mut acc2 = _mm256_setzero_ps();
        let mut acc3 = _mm256_setzero_ps();

        let ptr_a = a.as_ptr();
        let ptr_b = b.as_ptr();

        for i in (0..EMBEDDING_DIM).step_by(32) {
            let va0 = _mm256_loadu_ps(ptr_a.add(i));
            let vb0 = _mm256_loadu_ps(ptr_b.add(i));
            acc0 = _mm256_fmadd_ps(va0, vb0, acc0);

            let va1 = _mm256_loadu_ps(ptr_a.add(i + 8));
            let vb1 = _mm256_loadu_ps(ptr_b.add(i + 8));
            acc1 = _mm256_fmadd_ps(va1, vb1, acc1);

            let va2 = _mm256_loadu_ps(ptr_a.add(i + 16));
            let vb2 = _mm256_loadu_ps(ptr_b.add(i + 16));
            acc2 = _mm256_fmadd_ps(va2, vb2, acc2);

            let va3 = _mm256_loadu_ps(ptr_a.add(i + 24));
            let vb3 = _mm256_loadu_ps(ptr_b.add(i + 24));
            acc3 = _mm256_fmadd_ps(va3, vb3, acc3);
        }

        let sum01 = _mm256_add_ps(acc0, acc1);
        let sum23 = _mm256_add_ps(acc2, acc3);
        let total = _mm256_add_ps(sum01, sum23);

        let low = _mm256_castps256_ps128(total);
        let high = _mm256_extractf128_ps(total, 1);
        let sum128 = _mm_add_ps(low, high);

        let hadd1 = _mm_hadd_ps(sum128, sum128);
        let hadd2 = _mm_hadd_ps(hadd1, hadd1);

        _mm_cvtss_f32(hadd2)
    }
}

#[allow(dead_code)]
#[inline]
fn dot_product_512_scalar(a: &[f32; EMBEDDING_DIM], b: &[f32; EMBEDDING_DIM]) -> f32 {
    let mut sum = 0.0f32;
    for i in 0..EMBEDDING_DIM {
        sum += a[i] * b[i];
    }
    sum
}
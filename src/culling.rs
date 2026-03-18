use glam::{Mat4, Vec3, Vec4};
use rayon::prelude::*;
use crate::world::chunk::{CHUNK_W, CHUNK_H, CHUNK_D};

/// AABB чанка в мировых координатах
#[derive(Clone, Copy)]
pub struct ChunkAabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl ChunkAabb {
    pub fn from_chunk_pos(cx: i32, cz: i32) -> Self {
        let min = Vec3::new(
            cx as f32 * CHUNK_W as f32,
            0.0,
            cz as f32 * CHUNK_D as f32,
        );
        Self {
            min,
            max: min + Vec3::new(CHUNK_W as f32, CHUNK_H as f32, CHUNK_D as f32),
        }
    }
}

/// 6 плоскостей frustum (left, right, bottom, top, near, far)
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Извлекаем плоскости из view-proj матрицы (метод Gribb-Hartmann)
    pub fn from_view_proj(vp: &Mat4) -> Self {
        let m = vp.to_cols_array_2d();
        // m[col][row]
        let row = |r: usize| Vec4::new(m[0][r], m[1][r], m[2][r], m[3][r]);

        let r0 = row(0);
        let r1 = row(1);
        let r2 = row(2);
        let r3 = row(3);

        let planes = [
            (r3 + r0).normalize_or_zero(), // left
            (r3 - r0).normalize_or_zero(), // right
            (r3 + r1).normalize_or_zero(), // bottom
            (r3 - r1).normalize_or_zero(), // top
            (r3 + r2).normalize_or_zero(), // near
            (r3 - r2).normalize_or_zero(), // far
        ];

        Self { planes }
    }

    /// Проверяет AABB на пересечение с frustum
    /// true = видимый, false = за пределами (culled)
    #[inline]
    pub fn test_aabb(&self, aabb: &ChunkAabb) -> bool {
        for plane in &self.planes {
            // Позитивная вершина AABB (ближайшая к направлению нормали плоскости)
            let positive = Vec3::new(
                if plane.x >= 0.0 { aabb.max.x } else { aabb.min.x },
                if plane.y >= 0.0 { aabb.max.y } else { aabb.min.y },
                if plane.z >= 0.0 { aabb.max.z } else { aabb.min.z },
            );
            // Если позитивная вершина за плоскостью — весь AABB снаружи
            if plane.x * positive.x + plane.y * positive.y
             + plane.z * positive.z + plane.w < 0.0
            {
                return false;
            }
        }
        true
    }
}

/// Параллельный frustum culling по всем чанкам
/// Возвращает отсортированные по дистанции ключи видимых чанков
pub fn cull_chunks_parallel(
    chunk_keys: &[(i32, i32)],
    frustum:    &Frustum,
    cam_pos:    Vec3,
    pool:       &rayon::ThreadPool,
) -> Vec<(i32, i32)> {
    // Parallel filter + sort — rayon делает это эффективно
    let mut visible: Vec<(i32, i32, f32)> = pool.install(|| {
        chunk_keys
            .par_iter()
            .filter_map(|&(cx, cz)| {
                let aabb = ChunkAabb::from_chunk_pos(cx, cz);
                if frustum.test_aabb(&aabb) {
                    // Дистанция до центра чанка для сортировки
                    let center = (aabb.min + aabb.max) * 0.5;
                    let dist = (center - cam_pos).length_squared();
                    Some((cx, cz, dist))
                } else {
                    None
                }
            })
            .collect()
    });

    // Сортируем front-to-back — GPU отбрасывает дальние пиксели раньше
    visible.sort_unstable_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    visible.into_iter().map(|(cx, cz, _)| (cx, cz)).collect()
}
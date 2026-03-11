use glam::{Mat4, Vec3};

pub struct Camera {
    pub pos:   Vec3,   // позиция в мире
    pub yaw:   f32,    // поворот по горизонтали (градусы)
    pub pitch: f32,    // поворот по вертикали   (градусы)
    pub fov:   f32,    // поле зрения
    pub speed: f32,
    pub sensitivity: f32,
}

impl Camera {
    pub fn new(pos: Vec3) -> Self {
        Self {
            pos,
            yaw:   -90.0,
            pitch:   0.0,
            fov:    70.0,
            speed:   8.0,
            sensitivity: 0.15,
        }
    }

    /// Направление взгляда из yaw + pitch
    pub fn forward(&self) -> Vec3 {
        let yaw   = self.yaw.to_radians();
        let pitch = self.pitch.to_radians();
        Vec3::new(
            yaw.cos() * pitch.cos(),
            pitch.sin(),
            yaw.sin() * pitch.cos(),
        ).normalize()
    }

    /// View-матрица (куда смотрит камера)
    pub fn view(&self) -> Mat4 {
        Mat4::look_to_rh(self.pos, self.forward(), Vec3::Y)
    }

    /// Projection-матрица (перспектива)
    pub fn projection(&self, width: u32, height: u32) -> Mat4 {
        let aspect = width as f32 / height.max(1) as f32;
        // wgpu использует NDC с Z от 0 до 1 (не -1..1 как в OpenGL)
        Mat4::perspective_rh(self.fov.to_radians(), aspect, 0.1, 1000.0)
    }

    /// Итоговая матрица для шейдера
    pub fn view_proj(&self, width: u32, height: u32) -> Mat4 {
        self.projection(width, height) * self.view()
    }

    /// Вращение мышью (delta_x, delta_y в пикселях)
    pub fn rotate(&mut self, dx: f32, dy: f32) {
        self.yaw   += dx * self.sensitivity;
        self.pitch  -= dy * self.sensitivity;
        self.pitch   = self.pitch.clamp(-89.0, 89.0); // не переворачиваться
    }
}

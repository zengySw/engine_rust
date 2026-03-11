use glam::{Vec2, Vec3};

use crate::world::world::World;

pub struct Player {
    pub pos: Vec3, // feet position (center on XZ)
    vel: Vec3,
    pub on_ground: bool,
    pub radius: f32,
    pub height: f32,
    pub eye_height: f32,
    gravity: f32,
    jump_speed: f32,
    ground_accel: f32,
    air_accel: f32,
    ground_friction: f32,
}

impl Player {
    pub fn new(pos: Vec3) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            on_ground: false,
            radius: 0.30,
            height: 1.80,
            eye_height: 1.62,
            gravity: 30.0,
            jump_speed: 9.0,
            ground_accel: 55.0,
            air_accel: 18.0,
            ground_friction: 24.0,
        }
    }

    pub fn eye_pos(&self) -> Vec3 {
        self.pos + Vec3::Y * self.eye_height
    }

    pub fn simulate(
        &mut self,
        dt: f32,
        input: Vec2, // x = left/right, y = forward/back
        jump_pressed: bool,
        yaw_deg: f32,
        walk_speed: f32,
        world: &World,
    ) {
        let yaw = yaw_deg.to_radians();
        let forward = Vec3::new(yaw.cos(), 0.0, yaw.sin()).normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or_zero();

        let mut wish = forward * input.y + right * input.x;
        if wish.length_squared() > 1.0 {
            wish = wish.normalize();
        }

        let mut vel_h = Vec2::new(self.vel.x, self.vel.z);
        let target_h = Vec2::new(wish.x, wish.z) * walk_speed;
        let accel = if self.on_ground { self.ground_accel } else { self.air_accel };
        let max_step = accel * dt;

        let to_target = target_h - vel_h;
        let delta_len = to_target.length();
        if delta_len > max_step && delta_len > 0.0 {
            vel_h += to_target / delta_len * max_step;
        } else {
            vel_h = target_h;
        }

        if self.on_ground && target_h.length_squared() < 1e-4 {
            let speed = vel_h.length();
            if speed > 0.0 {
                let drop = self.ground_friction * dt;
                let new_speed = (speed - drop).max(0.0);
                vel_h *= new_speed / speed;
            }
        }

        if vel_h.length_squared() > walk_speed * walk_speed {
            vel_h = vel_h.normalize() * walk_speed;
        }
        self.vel.x = vel_h.x;
        self.vel.z = vel_h.y;

        if jump_pressed && self.on_ground {
            self.vel.y = self.jump_speed;
            self.on_ground = false;
        }

        self.vel.y -= self.gravity * dt;

        self.move_axis(0, self.vel.x * dt, world);
        self.move_axis(2, self.vel.z * dt, world);

        self.on_ground = false;
        let hit_y = self.move_axis(1, self.vel.y * dt, world);
        if hit_y {
            if self.vel.y < 0.0 {
                self.on_ground = true;
            }
            self.vel.y = 0.0;
        }

        // Failsafe in case we fall below loaded terrain.
        if self.pos.y < -64.0 {
            self.pos.y = 128.0;
            self.vel = Vec3::ZERO;
            self.on_ground = false;
        }
    }

    fn move_axis(&mut self, axis: usize, delta: f32, world: &World) -> bool {
        if delta.abs() <= f32::EPSILON {
            return false;
        }

        let start = self.pos;
        let mut target = start;
        set_axis(&mut target, axis, get_axis(start, axis) + delta);

        if !self.intersects_world(target, world) {
            self.pos = target;
            return false;
        }

        // Binary search to stop right before collision.
        let mut lo = 0.0;
        let mut hi = 1.0;
        for _ in 0..10 {
            let t = (lo + hi) * 0.5;
            let mut probe = start;
            set_axis(&mut probe, axis, get_axis(start, axis) + delta * t);
            if self.intersects_world(probe, world) {
                hi = t;
            } else {
                lo = t;
            }
        }

        let mut resolved = start;
        set_axis(&mut resolved, axis, get_axis(start, axis) + delta * lo);
        self.pos = resolved;
        true
    }

    fn intersects_world(&self, pos: Vec3, world: &World) -> bool {
        let eps = 0.001;
        let min = Vec3::new(pos.x - self.radius, pos.y, pos.z - self.radius);
        let max = Vec3::new(pos.x + self.radius, pos.y + self.height, pos.z + self.radius);

        let x0 = min.x.floor() as i32;
        let y0 = min.y.floor() as i32;
        let z0 = min.z.floor() as i32;
        let x1 = (max.x - eps).floor() as i32;
        let y1 = (max.y - eps).floor() as i32;
        let z1 = (max.z - eps).floor() as i32;

        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    if world.is_solid_at_world(x, y, z) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn get_axis(v: Vec3, axis: usize) -> f32 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

fn set_axis(v: &mut Vec3, axis: usize, value: f32) {
    match axis {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}

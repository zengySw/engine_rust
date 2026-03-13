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
    max_fall_speed: f32,
    step_height: f32,
    ground_snap_dist: f32,
    coyote_time: f32,
    jump_buffer_time: f32,
    coyote_timer: f32,
    jump_buffer_timer: f32,
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
            max_fall_speed: 54.0,
            step_height: 0.55,
            ground_snap_dist: 0.22,
            coyote_time: 0.09,
            jump_buffer_time: 0.10,
            coyote_timer: 0.0,
            jump_buffer_timer: 0.0,
        }
    }

    pub fn eye_pos(&self) -> Vec3 {
        self.pos + Vec3::Y * self.eye_height
    }

    pub fn teleport(&mut self, pos: Vec3) {
        self.pos = pos;
        self.vel = Vec3::ZERO;
        self.on_ground = false;
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
        let forward = Vec2::new(yaw.cos(), yaw.sin()).normalize_or_zero();
        let right = Vec2::new(-forward.y, forward.x);

        let mut wish = forward * input.y + right * input.x;
        if wish.length_squared() > 1.0 {
            wish = wish.normalize();
        }

        if jump_pressed {
            self.jump_buffer_timer = self.jump_buffer_time;
        } else {
            self.jump_buffer_timer = (self.jump_buffer_timer - dt).max(0.0);
        }

        if self.on_ground {
            self.coyote_timer = self.coyote_time;
        } else {
            self.coyote_timer = (self.coyote_timer - dt).max(0.0);
        }

        self.apply_horizontal_control(wish, walk_speed, dt);

        if self.jump_buffer_timer > 0.0 && self.coyote_timer > 0.0 {
            self.vel.y = self.jump_speed;
            self.on_ground = false;
            self.coyote_timer = 0.0;
            self.jump_buffer_timer = 0.0;
        }

        self.vel.y = (self.vel.y - self.gravity * dt).max(-self.max_fall_speed);

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

        if !self.on_ground && self.vel.y <= 0.0 {
            self.try_snap_to_ground(world);
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

        if let Some(step_pos) = self.try_step_move(axis, delta, world) {
            self.pos = step_pos;
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

    fn apply_horizontal_control(&mut self, wish: Vec2, walk_speed: f32, dt: f32) {
        let mut vel_h = Vec2::new(self.vel.x, self.vel.z);
        let has_input = wish.length_squared() > 1e-5;

        if has_input {
            let target_h = wish * walk_speed;
            let accel = if self.on_ground { self.ground_accel } else { self.air_accel };
            vel_h = move_towards_vec2(vel_h, target_h, accel * dt);
        } else if self.on_ground {
            let speed = vel_h.length();
            if speed > 0.0 {
                let drop = self.ground_friction * dt;
                let new_speed = (speed - drop).max(0.0);
                vel_h *= new_speed / speed;
            }
        }

        let max_h_speed = if self.on_ground { walk_speed } else { walk_speed * 1.15 };
        let max_h_speed_sq = max_h_speed * max_h_speed;
        if vel_h.length_squared() > max_h_speed_sq {
            vel_h = vel_h.normalize() * max_h_speed;
        }

        self.vel.x = vel_h.x;
        self.vel.z = vel_h.y;
    }

    fn try_step_move(&self, axis: usize, delta: f32, world: &World) -> Option<Vec3> {
        if axis == 1 || !self.on_ground {
            return None;
        }

        let start = self.pos;
        let mut elevated = start;
        elevated.y += self.step_height;
        if self.intersects_world(elevated, world) {
            return None;
        }

        let elevated_axis = get_axis(elevated, axis);
        set_axis(&mut elevated, axis, elevated_axis + delta);
        if self.intersects_world(elevated, world) {
            return None;
        }

        let mut below = elevated;
        below.y -= self.step_height + 0.01;
        if !self.intersects_world(below, world) {
            return None;
        }

        let mut lo = 0.0;
        let mut hi = self.step_height;
        for _ in 0..10 {
            let mid = (lo + hi) * 0.5;
            let mut probe = elevated;
            probe.y -= mid;
            if self.intersects_world(probe, world) {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        let mut resolved = elevated;
        resolved.y -= lo;
        Some(resolved)
    }

    fn try_snap_to_ground(&mut self, world: &World) {
        let start = self.pos;
        let mut down = start;
        down.y -= self.ground_snap_dist;

        if !self.intersects_world(down, world) {
            return;
        }

        let mut lo = 0.0;
        let mut hi = self.ground_snap_dist;
        for _ in 0..10 {
            let mid = (lo + hi) * 0.5;
            let mut probe = start;
            probe.y -= mid;
            if self.intersects_world(probe, world) {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        self.pos.y = start.y - lo;
        self.vel.y = 0.0;
        self.on_ground = true;
    }

    fn intersects_world(&self, pos: Vec3, world: &World) -> bool {
        if pos.y < 0.0 {
            return true;
        }

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

fn move_towards_vec2(current: Vec2, target: Vec2, max_delta: f32) -> Vec2 {
    let delta = target - current;
    let len = delta.length();
    if len <= max_delta || len <= f32::EPSILON {
        target
    } else {
        current + delta / len * max_delta
    }
}

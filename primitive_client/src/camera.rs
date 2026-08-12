use glam::{Mat4, Vec3};

/// Этап 4: "Реализация камеры, которая вращается и перемещается."
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,   // radians, rotation around Y (world up)
    pub pitch: f32, // radians, clamped to avoid flipping over the pole
    pub fov_y_radians: f32,
    pub aspect: f32,
    pub z_near: f32,
    pub z_far: f32,
}

const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

impl Camera {
    pub fn new(position: Vec3, aspect: f32) -> Self {
        Self {
            position,
            yaw: -std::f32::consts::FRAC_PI_2, // face -Z initially
            pitch: 0.0,
            fov_y_radians: 70f32.to_radians(),
            aspect,
            z_near: 0.05,
            z_far: 1000.0,
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize()
    }

    /// Forward projected onto the XZ plane, for WASD movement that doesn't
    /// climb/dive just because the player is looking up or down.
    pub fn forward_horizontal(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin()).normalize()
    }

    pub fn right_horizontal(&self) -> Vec3 {
        self.forward_horizontal().cross(Vec3::Y).normalize()
    }

    pub fn apply_mouse_delta(&mut self, dx: f32, dy: f32, sensitivity: f32) {
        self.yaw += dx * sensitivity;
        self.pitch = (self.pitch - dy * sensitivity).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    pub fn view_proj(&self) -> Mat4 {
        let view = Mat4::look_to_rh(self.position, self.forward(), Vec3::Y);
        let proj = Mat4::perspective_rh(self.fov_y_radians, self.aspect, self.z_near, self.z_far);
        proj * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_cannot_flip_over_the_pole() {
        let mut camera = Camera::new(Vec3::ZERO, 1.0);
        camera.apply_mouse_delta(0.0, -100_000.0, 0.01);
        assert!(camera.pitch < std::f32::consts::FRAC_PI_2);
        camera.apply_mouse_delta(0.0, 200_000.0, 0.01);
        assert!(camera.pitch > -std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn horizontal_forward_ignores_pitch() {
        let mut camera = Camera::new(Vec3::ZERO, 1.0);
        camera.pitch = 1.0;
        assert!(camera.forward_horizontal().y.abs() < 1e-6);
    }
}

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
    /// A purely visual offset added to the eye when building the view
    /// matrix: the running bob and the hit shake.
    ///
    /// Deliberately not folded into `position`. `position` is what the
    /// break/place ray starts from and what feeds the transform sent to
    /// the server, and a camera that jitters a few centimetres every
    /// frame would jitter both -- turning a cosmetic effect into aim
    /// wander and a stream of movement updates for a player standing
    /// still.
    pub shake: Vec3,
    /// Visual-only angular offset, as (pitch, yaw, roll) in radians.
    ///
    /// **This is the part that is actually seen.** Moving the eye a few
    /// centimetres barely changes the image at all: terrain is metres
    /// away, so a translation that small is almost pure parallax and
    /// reads as nothing happening. Rotating the view by a fraction of a
    /// degree moves every pixel on screen. A shake built only from
    /// translation is a shake nobody notices, which is exactly how the
    /// first attempt at this failed.
    ///
    /// Kept apart from `yaw`/`pitch` for the same reason `shake` is kept
    /// apart from `position`: those two are aim, and aim is the player's.
    pub shake_angles: Vec3,
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
            shake: Vec3::ZERO,
            shake_angles: Vec3::ZERO,
        }
    }

    /// Where the view is actually taken from, shake included.
    pub fn eye(&self) -> Vec3 {
        self.position + self.shake
    }

    /// The direction the *view* faces, which is aim plus shake.
    ///
    /// `forward` stays clean: it is what the break/place ray follows, so
    /// a shaking camera must not move the crosshair's target.
    fn view_forward(&self) -> Vec3 {
        let yaw = self.yaw + self.shake_angles.y;
        let pitch = (self.pitch + self.shake_angles.x)
            .clamp(-PITCH_LIMIT, PITCH_LIMIT);
        Vec3::new(
            yaw.cos() * pitch.cos(),
            pitch.sin(),
            yaw.sin() * pitch.cos(),
        )
        .normalize()
    }

    /// World up, rolled about the view axis.
    ///
    /// Roll is what sells both effects. `look_to_rh` with a hard-coded
    /// `Vec3::Y` can express a camera that nods and turns but never one
    /// that tilts, and a tilt is most of what a stride and a blow to the
    /// head actually look like.
    fn view_up(&self) -> Vec3 {
        let roll = self.shake_angles.z;
        if roll.abs() < 1e-6 {
            return Vec3::Y;
        }
        let axis = self.view_forward();
        let (sin, cos) = roll.sin_cos();
        // Rodrigues' rotation of Y about the view direction.
        Vec3::Y * cos + axis.cross(Vec3::Y) * sin + axis * axis.dot(Vec3::Y) * (1.0 - cos)
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
        let view = Mat4::look_to_rh(self.eye(), self.view_forward(), self.view_up());
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

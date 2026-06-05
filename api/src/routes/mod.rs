// routes/mod.rs

pub mod auth;
pub mod order;
// pub mod depth;

pub use auth::auth_routes;
pub use order::order_routes;
// pub use depth::depth_routes;



pub mod depth;

pub use depth::depth_routes;
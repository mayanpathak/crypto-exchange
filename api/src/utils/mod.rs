pub mod generatejwt;
pub mod hashpassword;
pub mod setcookie;

// Re-export important items explicitly (cleaner API)
pub use generatejwt::{generate_token, verify_token, Claims};
pub use hashpassword::{hash_password, verify_password};
pub use setcookie::{build_auth_cookie, clear_auth_cookie};


pub mod jwt { include!("./generatejwt.rs"); }
pub mod hash { include!("./hashpassword.rs"); }
pub mod cookie { include!("./setcookie.rs"); }    
/// Which claim identifies this token's intended client. Neither mode falls back to the other.
#[derive(Clone, Debug)]
pub enum ClientClaim {
    Audience,
    ClientId,
}

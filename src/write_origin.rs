use rocket::request::{FromRequest, Outcome};
use rocket_okapi::gen::OpenApiGenerator;
use rocket_okapi::request::{OpenApiFromRequest, RequestHeaderInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOrigin {
    Default,
    DbEntitySync,
}

impl WriteOrigin {
    pub fn is_db_entity_sync(&self) -> bool {
        matches!(self, WriteOrigin::DbEntitySync)
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for WriteOrigin {
    type Error = ();

    async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
        let origin = request.headers().get_one("X-Cedar-Write-Origin");
        match origin {
            Some("db-entity-sync") => Outcome::Success(WriteOrigin::DbEntitySync),
            _ => Outcome::Success(WriteOrigin::Default),
        }
    }
}

impl<'a> OpenApiFromRequest<'a> for WriteOrigin {
    fn from_request_input(
        _gen: &mut OpenApiGenerator,
        _name: String,
        _required: bool,
    ) -> rocket_okapi::Result<RequestHeaderInput> {
        // Optional header; keep docs minimal for now.
        Ok(RequestHeaderInput::None)
    }
}

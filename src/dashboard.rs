use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};

use crate::{auth::Admin, language::Language, Database, RocketResult};

#[get("/admin/dashboard")]
pub async fn dashboard(
    lang: Language,
    admin: Admin,
    mut db: Connection<Database>,
) -> RocketResult<Template> {
    let display_name = sqlx::query!("select display_name from admins where id = $1", &admin.id)
        .fetch_one(&mut *db)
        .await?
        .display_name;
    Ok(Template::render(
        "dashboard",
        context! { lang: lang.into_string(), display_name },
    ))
}

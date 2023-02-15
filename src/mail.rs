use std::collections::HashMap;

use fluent_templates::fluent_bundle::FluentValue;
use futures::TryStreamExt;
use lettre::{
    message::{
        header::{self, ContentTransferEncoding},
        IntoBody,
    },
    AsyncTransport, Message,
};
use map_macro::map;
use rocket_db_pools::Connection;
use rocket_dyn_templates::context;
use tera::Context;

use crate::{language::LOCALES, model::DateType, Config, Database, Mailer, MAIL_TEMPLATES};
use anyhow::anyhow;
use anyhow::Result;

pub enum MailBody<'a> {
    Raw(String),
    Template(&'a str, &'a Context),
}

pub async fn send_mail(
    config: &Config,
    mailer: &Mailer,
    to: &str,
    lang: &str,
    subject: &str,
    subject_args: Option<&HashMap<&str, &str>>,
    body: MailBody<'_>,
) -> Result<()> {
    let subject_args = match subject_args {
        Some(args) => {
            let mut new_map: HashMap<&str, FluentValue> = HashMap::new();
            for (key, value) in args {
                new_map.insert(key, (*value).into());
            }
            Some(new_map)
        }
        None => None,
    };
    let message_builder = Message::builder()
        .to(to.parse()?)
        .from(config.email_from_address.parse()?)
        .header(header::ContentType::TEXT_PLAIN)
        .subject(
            LOCALES
                .lookup_single_language(&lang.parse()?, subject, subject_args.as_ref())
                .ok_or_else(|| anyhow!(format!("Missing translation for {}!", subject)))?,
        );
    let message = match body {
        MailBody::Raw(text) => message_builder.body(text),
        MailBody::Template(key, context) => message_builder.body(
            MAIL_TEMPLATES
                .render(key, context)?
                .into_body(Some(ContentTransferEncoding::Base64)),
        ),
    }?;
    mailer.send(message).await?;
    Ok(())
}

pub async fn waiting_list_notify(
    db: &mut Connection<Database>,
    date_type: &str,
    config: &Config,
    mailer: &Mailer,
) -> Result<()> {
    let recipients: Vec<(String, String, String)> = sqlx::query!(
        r#"select email, lang, token
        from waiting_list
        where date_type = $1"#,
        &date_type
    )
    .fetch(&mut **db)
    .map_ok(|record| (record.email, record.lang, record.token))
    .try_collect::<Vec<(String, String, String)>>()
    .await?;

    for (email, lang, token) in recipients {
        let date_type = DateType::get_by_value(db, date_type, &lang).await?;
        let mail_header_args = map! {
            "datetype" => date_type.display_name.as_deref().unwrap()
        };
        send_mail(
            config,
            mailer,
            &email,
            &lang,
            "waiting-list",
            Some(&mail_header_args),
            MailBody::Template(
                "waiting-list-invite.tera",
                &Context::from_serialize(context! {
                    lang: &lang,
                    unsubscribe: format!(
                        "{}/waiting-list/unsubscribe/{}",
                        &config.web_address, &token
                    ),
                    link: format!("{}/dates/{}", &config.web_address, &date_type.value),
                })?,
            ),
        )
        .await?;
    }
    Ok(())
}

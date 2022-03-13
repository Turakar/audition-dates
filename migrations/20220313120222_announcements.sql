create type language as enum ('de', 'en');

create type announcement_position as enum ('general', 'choir', 'orchestra');

create table announcements (
    lang language not null,
    position announcement_position not null,
    description text not null,
    content text not null default '',
    primary key (lang, position)
);

insert into announcements (lang, position, description) values
    ('de', 'general', 'Allgemeine Ankündigung, die auf der Startseite angezeigt wird.'),
    ('en', 'general', 'General announcement that is shown on the start page.'),
    ('de', 'choir', 'Ankündigung, die auf der Seite angezeigt wird, wo Termine für den Chor angezeigt werden. Dieser Text wird auch in der Bestätigungs-Mail angezeigt.'),
    ('en', 'choir', 'Annoucement which is shown on the page where one can book choir audition dates. This text is also shown in the confirmation mail.'),
    ('de', 'orchestra', 'Ankündigung, die auf der Seite angezeigt wird, wo Termine für das Orchester angezeigt werden. Dieser Text wird auch in der Bestätigungs-Mail angezeigt.'),
    ('en', 'orchestra', 'Annoucement which is shown on the page where one can book orchestra audition dates. This text is also shown in the confirmation mail.')
;

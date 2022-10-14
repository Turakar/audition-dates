--------------------------------
-- SCHEMA
--------------------------------

-- create date type schema

create table date_types (
    id text primary key
);

create table date_types_translations (
    date_type text not null references date_types (id) on delete cascade,
    lang text not null,
    display_name text not null,
    primary key (date_type, lang)
);

-- create voice schema

create type voice_positions as enum ('booking', 'result');

create table voices (
    id serial primary key,
	value text not null,
    date_type text not null references date_types (id),
    position voice_positions not null,
	unique (value, date_type, position)
);

create table voices_translations (
    voice integer not null references voices (id) on delete cascade,
    lang text not null,
    display_name text not null,
    primary key (voice, lang)
);

--------------------------------
-- INITIALIZATION
--------------------------------

-- populate date types

insert into date_types (id)
values ('choir');

insert into date_types_translations (date_type, lang, display_name)
values ('choir', 'de', 'Chor'), ('choir', 'en', 'Choir');

-- choir booking voices

with voice as (
	insert into voices (value, date_type, position) values ('female', 'choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Weibliche Stimmlage'), ('en', 'Female voice')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('male', 'choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Männliche Stimmlage'), ('en', 'Male voice')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('soprano', 'choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Sopran'), ('en', 'Soprano')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('alto', 'choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Alt'), ('en', 'Alto')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('tenor', 'choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Tenor'), ('en', 'Tenor')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('bass', 'choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Bass'), ('en', 'Bass')) as sub (lang, display_name), voice;

-- choir result voices

with voice as (
	insert into voices (value, date_type, position) values ('soprano', 'choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Sopran'), ('en', 'Soprano')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('alto', 'choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Alt'), ('en', 'Alto')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('tenor', 'choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Tenor'), ('en', 'Tenor')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('bass', 'choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Bass'), ('en', 'Bass')) as sub (lang, display_name), voice;

--------------------------------
-- MIGRATION
--------------------------------

update bookings
set voice = (select id from voices where value = voice)::text;

alter table bookings
alter column voice type integer using voice::integer,
add constraint bookings_voice_fkey foreign key (voice) references voices (id) on delete cascade;

alter table dates
add constraint dates_date_type_fkey foreign key (date_type) references date_types (id) on delete cascade;

alter table waiting_list
add constraint waiting_list_date_type_fkey foreign key (date_type) references date_types (id) on delete cascade;

create table waiting_list (
    id serial primary key,
    date_type text not null,
    email varchar(50) unique not null,
    lang text not null,
    entered timestamp with time zone not null default now()
)

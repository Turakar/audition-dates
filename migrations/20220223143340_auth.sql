create table admins (
    id text primary key default gen_random_uuid(),
    display_name varchar(50) not null,
    email varchar(50) unique not null,
    password text not null,
    last_login timestamp with time zone
);

create table invites (
    token text primary key default gen_random_uuid(),
    email varchar(50) unique not null,
    created timestamp with time zone not null
);

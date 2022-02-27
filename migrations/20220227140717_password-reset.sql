create table password_resets (
    token text primary key default gen_random_uuid(),
    admin_id text not null,
    created timestamp with time zone default now()
)

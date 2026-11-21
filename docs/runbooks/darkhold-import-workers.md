# Darkhold import workers

`darkhold.service` still runs one embedded worker. Extra instances of
`deploy/darkhold-import-worker@.service` add queue throughput, not pressure on one
archive: `jobs::claim_next` refuses a second leased `import` for any
`import_jobs.source_key`, no matter how many worker processes exist.

## Install

On thinkcentre:

```
cp deploy/darkhold-import-worker@.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now darkhold-import-worker@2.service darkhold-import-worker@3.service darkhold-import-worker@4.service
```

Each instance gets a random worker id from `WorkerOptions::default()`; they never
share one, so a lost lease remains attributable.

## Verify

```
systemctl --user is-active darkhold-import-worker@2.service
sudo -u postgres psql -d darkhold -Atc "select source_key,count(*) from jobs j join import_jobs ij on ij.job_id=j.id where j.state='leased' group by 1 having count(*)>1;"
```

The second command must print no rows. A row means the per-source guard is not
running in the deployed binary; stop the extra workers before investigating.

## Pace

The guard is one leased import per source, not a per-site rate limit. TheForce
publishes `Crawl-delay: 10` and its adapter declares a 10-second minimum
interval; the fetcher still owns pacing. Do not raise worker count to work around
a 429 or a crawl-delay.

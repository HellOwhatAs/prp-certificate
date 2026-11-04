"""Fetch the optional manual CI fixture ZIP without exposing its private URL.

PRP_REGRESSION_FIXTURE_URL and PRP_REGRESSION_FIXTURE_SHA256 are repository
secrets. The archive must contain exactly job-q1.json, job-q2.json, job-q3.json.
Only the manual full regression workflow calls this script.
"""
import hashlib
import io
import json
import os
from pathlib import Path
import re
import sys
import urllib.request
import zipfile

url = os.environ.get('PRP_REGRESSION_FIXTURE_URL', '')
expected = os.environ.get('PRP_REGRESSION_FIXTURE_SHA256', '')
if not url.startswith('https://') or not re.fullmatch('[0-9a-f]{64}', expected):
    raise SystemExit('Full Q1/Q2/Q3 regression requires configured PRP_REGRESSION_FIXTURE_URL and PRP_REGRESSION_FIXTURE_SHA256 secrets; no tests were run.')
if len(sys.argv) != 2:
    raise SystemExit('Usage: python scripts/fetch-regression-fixtures.py OUTPUT_DIRECTORY')
limit = 8 * 1024 * 1024
try:
    with urllib.request.urlopen(url, timeout=60) as response:
        if not response.geturl().startswith('https://'):
            raise ValueError('non-HTTPS redirect')
        data = response.read(limit + 1)
except Exception:
    raise SystemExit('Cannot download the private fixture archive; the secret URL was not logged.') from None
if len(data) > limit or hashlib.sha256(data).hexdigest() != expected:
    raise SystemExit('Private fixture archive is too large or has the wrong SHA-256.')
required = {f'job-q{q}.json' for q in (1, 2, 3)}
try:
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        if len(entries) != 3 or {entry.filename for entry in entries} != required or any(entry.file_size > limit for entry in entries):
            raise ValueError('invalid archive members')
        jobs = {}
        for q in (1, 2, 3):
            name = f'job-q{q}.json'
            job = json.loads(archive.read(name))
            if job.get('q') != q or any(not isinstance(job['files'][key], str) or not job['files'][key] for key in ('routes', 'assignments')):
                raise ValueError('invalid fixture')
            jobs[name] = {'action':'prove', 'q':q, 'team':'Private regression fixture', 'files':{key:job['files'][key] for key in ('routes', 'assignments')}}
except Exception:
    raise SystemExit('Private archive must contain valid job-q1.json, job-q2.json and job-q3.json at its root; no tests were run.') from None
output = Path(sys.argv[1])
output.mkdir(parents=True, exist_ok=True)
for name, job in jobs.items():
    (output / name).write_text(json.dumps(job))
print('All three private fixture files are present; archive digest verified. No solution content was logged.')

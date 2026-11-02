import json,csv,pathlib,datetime,hashlib,sys
root=pathlib.Path(sys.argv[1]) if len(sys.argv)>1 else pathlib.Path('week0727')
scenario=json.loads((root/'organizer/configs/scenario.json').read_text())
locations=scenario['locations']['airports']+scenario['locations']['platforms']
planes=[f'{a}-{t}-H{i:02}' for a,types in scenario['fleet'].items() for t,n in types.items() for i in range(1,n+1)]
def minutes(s): return int((datetime.datetime.strptime(s,'%Y-%m-%d %H:%M')-datetime.datetime(2026,8,3)).total_seconds()/60)
people={}
for q in (1,2,3):
 rows=list(csv.DictReader((root/f'participant_package/data/peopleQ{q}.csv').open()))
 people[q]=[dict(id=r['person_id'],origin=55 if r['origin_id']=='LAND' else locations.index(r['origin_id']),destination=55 if r['destination_id']=='LAND' else locations.index(r['destination_id']),earliest=minutes(r['earliest_pickup_time']) if q==3 else 0,latest=minutes(r['latest_arrival_time']) if q==3 else 10080,task=r.get('task_type','all')) for r in rows]
dist=[list(map(int,list(r.values())[1:])) for r in csv.DictReader((root/'participant_package/data/distances.csv').open())]
data=dict(locations=locations,planes=planes,planeType=[int(p[5])-1 for p in planes],planeHome=[locations.index(p[:3]) for p in planes],refuel=[locations.index(p) for p in scenario['locations']['refuel_platforms']],distance=dist,people=people,capacity=[12,16,19],speed=[250,220,190],fuelRate10=[34,25,29],tank10=[10000,11500,16000],reserve10=[1500,1500,2000])
raw=json.dumps(data,ensure_ascii=False,separators=(',',':'))
data['datasetId']=hashlib.sha256(raw.encode()).hexdigest()
pathlib.Path('public/data/instance.json').write_text(json.dumps(data,ensure_ascii=False,separators=(',',':')))
print({q:len(p) for q,p in people.items()},data['datasetId'])

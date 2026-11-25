"""Shared phase gates used by live driver and injected pure tests."""
class Coordinator:
    def __init__(self, outcome, save):
        self.outcome, self.save = outcome, save
        self.completed = set()
    def phase(self, name, operation, needs=()):
        if not set(needs) <= self.completed:
            raise RuntimeError('Missing phase prerequisites: '+name)
        row={'name':name,'status':'RUNNING'}
        self.outcome['phases'].append(row); self.save()
        try:
            value=operation()
            row['status']='COMPLETED' if isinstance(value,dict) and value.get('status')=='COMPLETED' else 'PASS';self.completed.add(name)
            return value
        except Exception as error:
            row.update(status='FAIL',error=str(error),errorType=type(error).__name__)
            raise
        finally:self.save()
